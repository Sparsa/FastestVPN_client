use crate::backend::api::{FastestVpnApiClient, UserData};
use crate::backend::config::{VpnProfile, VpnProtocol};
use crate::backend::elevation::ElevationManager;
use crate::backend::tunnel::{NetworkStats, TunnelController, TunnelError, TunnelState};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug)]
pub enum ServiceCommand {
    Connect {
        profile: VpnProfile,
        username: String,
        password: String,
    },
    Disconnect,
    LoginApi {
        username: String,
        password: String,
    },
    CheckInterface,
    InstallPolkitPolicy,
}

#[derive(Debug)]
pub enum ServiceEvent {
    StateChanged(TunnelState),
    StatsUpdated(NetworkStats),
    Log(LogEntry),
    /// Account details plus the session token on success.
    LoginResult(Result<(UserData, Option<String>), String>),
    PolkitInstalled(Result<String, String>),
}

pub struct VpnBackendService {
    cmd_sender: Sender<ServiceCommand>,
    event_receiver: Receiver<ServiceEvent>,
    pub current_state: Arc<Mutex<TunnelState>>,
    pub latest_stats: Arc<Mutex<NetworkStats>>,
    pub logs: Arc<Mutex<Vec<LogEntry>>>,
}

impl VpnBackendService {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel::<ServiceCommand>();
        let (event_tx, event_rx) = channel::<ServiceEvent>();

        let current_state = Arc::new(Mutex::new(TunnelState::Disconnected));
        let latest_stats = Arc::new(Mutex::new(NetworkStats::default()));
        let logs = Arc::new(Mutex::new(Vec::new()));

        let state_worker = Arc::clone(&current_state);
        let stats_worker = Arc::clone(&latest_stats);
        let logs_worker = Arc::clone(&logs);

        // Spawn background worker thread
        thread::Builder::new()
            .name("fastestvpn-backend-worker".to_string())
            .spawn(move || {
                let controller = TunnelController::default();
                let mut last_sample_time = Instant::now();
                let mut prev_rx: u64 = 0;
                let mut prev_tx: u64 = 0;
                let mut active_iface: Option<(String, VpnProtocol)> = None;

                let log = |level: LogLevel, msg: &str| {
                    let entry = LogEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        level,
                        message: msg.to_string(),
                    };
                    if let Ok(mut lock) = logs_worker.lock() {
                        if lock.len() > 1000 {
                            lock.remove(0);
                        }
                        lock.push(entry.clone());
                    }
                    let _ = event_tx.send(ServiceEvent::Log(entry));
                };

                log(LogLevel::Info, "FastestVPN Dual-Stack Service (WireGuard + OpenVPN) initialized.");

                // Initial check for active leftover interfaces
                if let Some((iface, proto)) = controller.active_interface() {
                    log(
                        LogLevel::Warning,
                        &format!("Detected existing active interface '{iface}' ({proto})"),
                    );
                    let state = TunnelState::Connected {
                        profile_name: "Active VPN Tunnel".to_string(),
                        endpoint: "Unknown".to_string(),
                        protocol: proto,
                        interface: iface.clone(),
                        connected_at: Instant::now(),
                    };
                    active_iface = Some((iface, proto));
                    *state_worker.lock().unwrap() = state.clone();
                    let _ = event_tx.send(ServiceEvent::StateChanged(state));
                }

                loop {
                    match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(ServiceCommand::Connect { profile, username, password }) => {
                            log(
                                LogLevel::Info,
                                &format!(
                                    "Connecting to '{}' via {} ({})...",
                                    profile.name,
                                    profile.protocol,
                                    profile.display_endpoint()
                                ),
                            );

                            let connecting_state = TunnelState::Connecting {
                                server_name: profile.name.clone(),
                                protocol: profile.protocol,
                                started_at: Instant::now(),
                            };
                            *state_worker.lock().unwrap() = connecting_state.clone();
                            let _ = event_tx.send(ServiceEvent::StateChanged(connecting_state));

                            let connect_res = match profile.protocol {
                                VpnProtocol::WireGuard => {
                                    controller.start_wireguard_tunnel(&profile).map(|_| {
                                        (controller.wg_interface_name().to_string(), VpnProtocol::WireGuard)
                                    })
                                }
                                VpnProtocol::OpenVpnUdp | VpnProtocol::OpenVpnTcp => {
                                    controller
                                        .start_openvpn_tunnel(&profile, &username, &password)
                                        .map(|_| {
                                            (controller.ovpn_interface_name().to_string(), profile.protocol)
                                        })
                                }
                            };

                            match connect_res {
                                Ok((iface, proto)) => {
                                    log(
                                        LogLevel::Success,
                                        &format!("Connected to {} on interface {} using {proto}", profile.name, iface),
                                    );
                                    let connected_state = TunnelState::Connected {
                                        profile_name: profile.name.clone(),
                                        endpoint: profile.display_endpoint().to_string(),
                                        protocol: proto,
                                        interface: iface.clone(),
                                        connected_at: Instant::now(),
                                    };
                                    *state_worker.lock().unwrap() = connected_state.clone();
                                    active_iface = Some((iface.clone(), proto));
                                    let _ = event_tx.send(ServiceEvent::StateChanged(connected_state));

                                    // Reset traffic counters
                                    if let Some(stats) = controller.read_statistics(&iface) {
                                        prev_rx = stats.rx_bytes;
                                        prev_tx = stats.tx_bytes;
                                        last_sample_time = Instant::now();
                                    }
                                }
                                Err(err) => {
                                    let err_msg = format!("{err}");
                                    log(LogLevel::Error, &format!("Connection failed: {err_msg}"));
                                    let error_state = TunnelState::Error { message: err_msg };
                                    *state_worker.lock().unwrap() = error_state.clone();
                                    let _ = event_tx.send(ServiceEvent::StateChanged(error_state));
                                }
                            }
                        }
                        Ok(ServiceCommand::Disconnect) => {
                            log(LogLevel::Info, "Disconnecting VPN tunnel...");
                            let disconnecting = TunnelState::Disconnecting;
                            *state_worker.lock().unwrap() = disconnecting.clone();
                            let _ = event_tx.send(ServiceEvent::StateChanged(disconnecting));

                            match controller.stop_active() {
                                Ok(()) => log(LogLevel::Info, "Tunnel terminated successfully."),
                                Err(TunnelError::NotConnected) => {
                                    log(LogLevel::Info, "No active tunnel to terminate.")
                                }
                                Err(e) => log(LogLevel::Error, &format!("Disconnect failed: {e}")),
                            }
                            let disconnected = TunnelState::Disconnected;
                            *state_worker.lock().unwrap() = disconnected.clone();
                            active_iface = None;
                            let _ = event_tx.send(ServiceEvent::StateChanged(disconnected));
                        }
                        Ok(ServiceCommand::LoginApi { username, password }) => {
                            log(
                                LogLevel::Info,
                                &format!("Authenticating account '{username}' via FastestVPN API (api.fastestvpn.com)..."),
                            );

                            let api = FastestVpnApiClient::new();
                            match api.authenticate(&username, &password) {
                                Ok(auth_res) => {
                                    let user = auth_res.user_data();
                                    let token = auth_res.session_token();
                                    log(
                                        LogLevel::Info,
                                        &format!("API response data fields: {:?}", auth_res.data_keys()),
                                    );
                                    let status = user.status.clone().unwrap_or_else(|| "Active".to_string());
                                    let expiry = user.expiry_date.clone().unwrap_or_else(|| "Never".to_string());

                                    log(
                                        LogLevel::Success,
                                        &format!("FastestVPN API Login verified! Status: {status}, Expiry: {expiry}"),
                                    );

                                    let _ = event_tx.send(ServiceEvent::LoginResult(Ok((user, token))));
                                }
                                Err(e) => {
                                    let err_msg = format!("{e}");
                                    log(LogLevel::Error, &format!("API Login failed: {err_msg}"));
                                    let _ = event_tx.send(ServiceEvent::LoginResult(Err(err_msg)));
                                }
                            }
                        }
                        Ok(ServiceCommand::CheckInterface) => {
                            if let Some((ref iface, _)) = active_iface {
                                if !controller.is_interface_active(iface) {
                                    log(LogLevel::Warning, "VPN interface was dropped unexpectedly.");
                                    let disconnected = TunnelState::Disconnected;
                                    *state_worker.lock().unwrap() = disconnected.clone();
                                    active_iface = None;
                                    let _ = event_tx.send(ServiceEvent::StateChanged(disconnected));
                                }
                            }
                        }
                        Ok(ServiceCommand::InstallPolkitPolicy) => {
                            log(LogLevel::Info, "Installing PolicyKit policy for wg-quick and openvpn...");
                            let policy_xml = ElevationManager::generate_polkit_policy();
                            let target = "/usr/share/polkit-1/actions/com.fastestvpn.wgquick.policy";
                            let tmp_path = format!("/tmp/fastestvpn_policy_{}.xml", std::process::id());

                            if let Err(e) = std::fs::write(&tmp_path, policy_xml) {
                                let res = format!("Failed to write temp policy: {e}");
                                log(LogLevel::Error, &res);
                                let _ = event_tx.send(ServiceEvent::PolkitInstalled(Err(res)));
                                continue;
                            }

                            let mut cmd = std::process::Command::new("pkexec");
                            cmd.args(["cp", &tmp_path, target]);
                            match cmd.status() {
                                Ok(status) if status.success() => {
                                    let msg = format!("Policy installed to {target}. Root elevation will now authenticate seamlessly.");
                                    log(LogLevel::Success, &msg);
                                    let _ = event_tx.send(ServiceEvent::PolkitInstalled(Ok(msg)));
                                }
                                Ok(status) => {
                                    let msg = format!("Policy install failed with exit code: {:?}", status.code());
                                    log(LogLevel::Error, &msg);
                                    let _ = event_tx.send(ServiceEvent::PolkitInstalled(Err(msg)));
                                }
                                Err(e) => {
                                    let msg = format!("pkexec spawn error: {e}");
                                    log(LogLevel::Error, &msg);
                                    let _ = event_tx.send(ServiceEvent::PolkitInstalled(Err(msg)));
                                }
                            }
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            break;
                        }
                    }

                    // Sample stats if connected
                    if let Some((ref iface, _)) = active_iface {
                        if let Some(mut stats) = controller.read_statistics(iface) {
                            let now = Instant::now();
                            let elapsed = now.duration_since(last_sample_time).as_secs_f64();
                            if elapsed >= 0.8 {
                                let rx_diff = stats.rx_bytes.saturating_sub(prev_rx);
                                let tx_diff = stats.tx_bytes.saturating_sub(prev_tx);

                                stats.rx_rate_bps = (rx_diff as f64) / elapsed;
                                stats.tx_rate_bps = (tx_diff as f64) / elapsed;

                                prev_rx = stats.rx_bytes;
                                prev_tx = stats.tx_bytes;
                                last_sample_time = now;

                                *stats_worker.lock().unwrap() = stats.clone();
                                let _ = event_tx.send(ServiceEvent::StatsUpdated(stats));
                            }
                        } else {
                            log(LogLevel::Warning, "VPN interface vanished.");
                            let state = TunnelState::Disconnected;
                            *state_worker.lock().unwrap() = state.clone();
                            active_iface = None;
                            let _ = event_tx.send(ServiceEvent::StateChanged(state));
                        }
                    }
                }

                // Cleanup on thread exit
                let _ = controller.stop_active();
            })
            .expect("Failed to spawn backend worker thread");

        Self {
            cmd_sender: cmd_tx,
            event_receiver: event_rx,
            current_state,
            latest_stats,
            logs,
        }
    }

    pub fn send_command(&self, cmd: ServiceCommand) {
        let _ = self.cmd_sender.send(cmd);
    }

    pub fn poll_events<F>(&self, mut callback: F)
    where
        F: FnMut(ServiceEvent),
    {
        while let Ok(event) = self.event_receiver.try_recv() {
            callback(event);
        }
    }
}
