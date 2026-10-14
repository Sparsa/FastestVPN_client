use crate::backend::config::VpnProfile;
use crate::backend::elevation::ElevationManager;
use crate::backend::tunnel::{NetworkStats, TunnelController, TunnelState};
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
    Connect(VpnProfile),
    Disconnect,
    CheckInterface,
    InstallPolkitPolicy,
}

#[derive(Debug)]
pub enum ServiceEvent {
    StateChanged(TunnelState),
    StatsUpdated(NetworkStats),
    Log(LogEntry),
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
                let mut _active_profile: Option<VpnProfile> = None;

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

                log(LogLevel::Info, "FastestVPN Backend Service initialized.");

                // Initial check: if an interface is already lingering, detect it
                if controller.is_interface_active() {
                    log(
                        LogLevel::Warning,
                        &format!("Detected existing active interface '{}'", controller.interface_name()),
                    );
                    let state = TunnelState::Connected {
                        profile_name: "Active WireGuard Tunnel".to_string(),
                        endpoint: "Unknown".to_string(),
                        interface: controller.interface_name().to_string(),
                        connected_at: Instant::now(),
                    };
                    *state_worker.lock().unwrap() = state.clone();
                    let _ = event_tx.send(ServiceEvent::StateChanged(state));
                }

                loop {
                    // Non-blocking or short timeout recv
                    match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(ServiceCommand::Connect(profile)) => {
                            log(
                                LogLevel::Info,
                                &format!("Initiating tunnel connection to '{}' ({})...", profile.name, profile.display_endpoint()),
                            );

                            let connecting_state = TunnelState::Connecting {
                                server_name: profile.name.clone(),
                                started_at: Instant::now(),
                            };
                            *state_worker.lock().unwrap() = connecting_state.clone();
                            let _ = event_tx.send(ServiceEvent::StateChanged(connecting_state));

                            // Perform root elevated connection
                            match controller.start_tunnel(&profile) {
                                Ok(()) => {
                                    log(
                                        LogLevel::Success,
                                        &format!("Connected successfully to {} on interface {}", profile.name, controller.interface_name()),
                                    );
                                    let connected_state = TunnelState::Connected {
                                        profile_name: profile.name.clone(),
                                        endpoint: profile.display_endpoint().to_string(),
                                        interface: controller.interface_name().to_string(),
                                        connected_at: Instant::now(),
                                    };
                                    *state_worker.lock().unwrap() = connected_state.clone();
                                    _active_profile = Some(profile);
                                    let _ = event_tx.send(ServiceEvent::StateChanged(connected_state));

                                    // Reset stats counters
                                    if let Some(stats) = controller.read_statistics() {
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
                            log(LogLevel::Info, "Disconnecting tunnel...");
                            let disconnecting = TunnelState::Disconnecting;
                            *state_worker.lock().unwrap() = disconnecting.clone();
                            let _ = event_tx.send(ServiceEvent::StateChanged(disconnecting));

                            match controller.stop_tunnel() {
                                Ok(()) => {
                                    log(LogLevel::Info, "Tunnel successfully terminated.");
                                    let disconnected = TunnelState::Disconnected;
                                    *state_worker.lock().unwrap() = disconnected.clone();
                                    _active_profile = None;
                                    let _ = event_tx.send(ServiceEvent::StateChanged(disconnected));
                                }
                                Err(err) => {
                                    let err_msg = format!("Failed to stop tunnel: {err}");
                                    log(LogLevel::Error, &err_msg);
                                    let error_state = TunnelState::Error { message: err_msg };
                                    *state_worker.lock().unwrap() = error_state.clone();
                                    let _ = event_tx.send(ServiceEvent::StateChanged(error_state));
                                }
                            }
                        }
                        Ok(ServiceCommand::CheckInterface) => {
                            let is_active = controller.is_interface_active();
                            let mut current = state_worker.lock().unwrap();
                            if !is_active && matches!(*current, TunnelState::Connected { .. }) {
                                log(LogLevel::Warning, "Tunnel interface was dropped unexpectedly.");
                                *current = TunnelState::Disconnected;
                                let _ = event_tx.send(ServiceEvent::StateChanged(TunnelState::Disconnected));
                            }
                        }
                        Ok(ServiceCommand::InstallPolkitPolicy) => {
                            log(LogLevel::Info, "Attempting to install Polkit policy via pkexec...");
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
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            // Periodic monitoring step
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            // UI closed
                            break;
                        }
                    }

                    // If currently connected, sample traffic rates and verify interface
                    let is_connected = matches!(*state_worker.lock().unwrap(), TunnelState::Connected { .. });
                    if is_connected {
                        if let Some(mut stats) = controller.read_statistics() {
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
                            // Interface is missing!
                            log(LogLevel::Warning, "WireGuard interface vanished.");
                            let state = TunnelState::Disconnected;
                            *state_worker.lock().unwrap() = state.clone();
                            let _ = event_tx.send(ServiceEvent::StateChanged(state));
                        }
                    }
                }

                // Cleanup on thread exit if connected
                if controller.is_interface_active() {
                    let _ = controller.stop_tunnel();
                }
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

    /// Process pending events sent by the worker thread (called on each UI frame).
    pub fn poll_events<F>(&self, mut callback: F)
    where
        F: FnMut(ServiceEvent),
    {
        while let Ok(event) = self.event_receiver.try_recv() {
            callback(event);
        }
    }
}
