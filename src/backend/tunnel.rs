use crate::backend::config::{VpnProfile, VpnProtocol, PLACEHOLDER_PRIVATE_KEY};
use crate::backend::elevation::{validate_interface_name, ElevationError, ElevationManager};
use crate::backend::openvpn::generate_fastestvpn_ovpn;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

pub const DEFAULT_WG_INTERFACE: &str = "wg-fastestvpn";
pub const DEFAULT_OPENVPN_INTERFACE: &str = "fastest-tun";

/// Polls (500 ms apart) to wait for the OpenVPN tun device after daemonizing.
const OPENVPN_UP_TIMEOUT_POLLS: u32 = 40;

#[derive(Error, Debug, Clone)]
pub enum TunnelError {
    #[error("Elevation failed: {0}")]
    Elevation(#[from] ElevationError),
    #[error("Tunnel already active on interface {0}")]
    AlreadyConnected(String),
    #[error("No active tunnel found to disconnect")]
    NotConnected,
    #[error("Credentials missing: username and password required for OpenVPN")]
    MissingCredentials,
    #[error("I/O error during tunnel operation: {0}")]
    Io(String),
    #[error("Tunnel verification failed: interface {0} did not come up")]
    InterfaceVerificationFailed(String),
    #[error("Invalid WireGuard profile: {0}")]
    InvalidProfile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting {
        server_name: String,
        protocol: VpnProtocol,
        started_at: Instant,
    },
    Connected {
        profile_name: String,
        endpoint: String,
        protocol: VpnProtocol,
        interface: String,
        connected_at: Instant,
    },
    Disconnecting,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_bps: f64,
    pub tx_rate_bps: f64,
    pub last_handshake: Option<String>,
}

pub struct TunnelController {
    elevation: ElevationManager,
    wg_interface: String,
    ovpn_interface: String,
    runtime_dir: PathBuf,
}

impl Default for TunnelController {
    fn default() -> Self {
        Self::new(DEFAULT_WG_INTERFACE, DEFAULT_OPENVPN_INTERFACE)
    }
}

impl TunnelController {
    pub fn new(wg_interface: &str, ovpn_interface: &str) -> Self {
        let elevation = ElevationManager::new();
        let runtime_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("fastestvpn-gui")
            .join("runtime");

        Self {
            elevation,
            wg_interface: wg_interface.to_string(),
            ovpn_interface: ovpn_interface.to_string(),
            runtime_dir,
        }
    }

    pub fn wg_interface_name(&self) -> &str {
        &self.wg_interface
    }

    pub fn ovpn_interface_name(&self) -> &str {
        &self.ovpn_interface
    }

    pub fn elevation_manager(&self) -> &ElevationManager {
        &self.elevation
    }

    pub fn is_interface_active(&self, iface: &str) -> bool {
        let sys_path = Path::new("/sys/class/net").join(iface);
        sys_path.exists()
    }

    /// Check if either WireGuard or OpenVPN interface is active
    pub fn active_interface(&self) -> Option<(String, VpnProtocol)> {
        if self.is_interface_active(&self.wg_interface) {
            return Some((self.wg_interface.clone(), VpnProtocol::WireGuard));
        }
        if self.is_interface_active(&self.ovpn_interface) {
            return Some((self.ovpn_interface.clone(), VpnProtocol::OpenVpnUdp));
        }
        None
    }

    fn ovpn_pid_path(&self) -> PathBuf {
        self.runtime_dir.join(format!("{}.pid", self.ovpn_interface))
    }

    fn ovpn_auth_path(&self) -> PathBuf {
        self.runtime_dir.join("openvpn_auth.txt")
    }

    /// Starts a WireGuard tunnel for the specified profile.
    pub fn start_wireguard_tunnel(&self, profile: &VpnProfile) -> Result<(), TunnelError> {
        validate_interface_name(&self.wg_interface)?;

        // Refuse to bring up a tunnel that cannot route traffic: an interface with a
        // bogus key would come up and report "Protected" while blackholing traffic.
        if profile.config.interface.private_key == PLACEHOLDER_PRIVATE_KEY {
            return Err(TunnelError::InvalidProfile(
                "no WireGuard PrivateKey configured. Add your key under Servers → Account & Login, \
                 or use the OpenVPN protocol with your FastestVPN username and password"
                    .to_string(),
            ));
        }
        profile
            .config
            .validate()
            .map_err(|e| TunnelError::InvalidProfile(e.to_string()))?;

        if self.is_interface_active(&self.wg_interface) {
            return Err(TunnelError::AlreadyConnected(self.wg_interface.clone()));
        }

        fs::create_dir_all(&self.runtime_dir)
            .map_err(|e| TunnelError::Io(format!("Failed to create runtime dir: {e}")))?;

        let config_path = self.runtime_dir.join(format!("{}.conf", self.wg_interface));
        let conf_content = profile.config.to_conf_string();

        ElevationManager::write_secure_config(&config_path, &conf_content)?;

        match self.elevation.wg_quick_up(&config_path) {
            Ok(_) => {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if !self.is_interface_active(&self.wg_interface) {
                    let _ = self.elevation.wg_quick_down(config_path.to_str().unwrap_or(""));
                    return Err(TunnelError::InterfaceVerificationFailed(self.wg_interface.clone()));
                }
                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&config_path);
                Err(TunnelError::Elevation(e))
            }
        }
    }

    /// Starts an OpenVPN tunnel using FastestVPN username and password.
    pub fn start_openvpn_tunnel(
        &self,
        profile: &VpnProfile,
        username: &str,
        password: &str,
    ) -> Result<(), TunnelError> {
        if username.trim().is_empty() || password.trim().is_empty() {
            return Err(TunnelError::MissingCredentials);
        }

        validate_interface_name(&self.ovpn_interface)?;

        if self.is_interface_active(&self.ovpn_interface) {
            return Err(TunnelError::AlreadyConnected(self.ovpn_interface.clone()));
        }

        fs::create_dir_all(&self.runtime_dir)
            .map_err(|e| TunnelError::Io(format!("Failed to create runtime dir: {e}")))?;

        // 1. Write temporary auth file (username \n password) with 0600 permissions
        let auth_path = self.ovpn_auth_path();
        let pid_path = self.ovpn_pid_path();
        let _ = fs::remove_file(&pid_path);
        let auth_content = format!("{}\n{}\n", username.trim(), password.trim());
        ElevationManager::write_secure_config(&auth_path, &auth_content)?;

        // 2. Prepare .ovpn config file
        let ovpn_path = self.runtime_dir.join(format!("{}.ovpn", self.ovpn_interface));
        let ovpn_content = if let Some(ref path) = profile.file_path {
            if path.extension().and_then(|s| s.to_str()) == Some("ovpn") {
                fs::read_to_string(path).unwrap_or_else(|_| generate_fastestvpn_ovpn("de-01.jumptoserver.com", profile.protocol))
            } else {
                let host = profile.display_endpoint().split(':').next().unwrap_or("de-01.jumptoserver.com");
                generate_fastestvpn_ovpn(host, profile.protocol)
            }
        } else {
            let host = profile.display_endpoint().split(':').next().unwrap_or("de-01.jumptoserver.com");
            generate_fastestvpn_ovpn(host, profile.protocol)
        };

        ElevationManager::write_secure_config(&ovpn_path, &ovpn_content)?;

        // 3. Launch elevated OpenVPN daemon
        match self.elevation.openvpn_up(&ovpn_path, &auth_path, &pid_path, &self.ovpn_interface) {
            Ok(_) => {
                // `--daemon` returns before the TLS handshake and auth finish; the tun
                // device only appears once the server has pushed its config.
                for _ in 0..OPENVPN_UP_TIMEOUT_POLLS {
                    if self.is_interface_active(&self.ovpn_interface) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }

                // OpenVPN has read (and cached) the credentials by now; don't leave the
                // plaintext password on disk for the lifetime of the tunnel.
                let _ = fs::remove_file(&auth_path);

                if !self.is_interface_active(&self.ovpn_interface) {
                    let _ = self.elevation.openvpn_down(&self.ovpn_interface, &pid_path);
                    let _ = fs::remove_file(&ovpn_path);
                    return Err(TunnelError::InterfaceVerificationFailed(self.ovpn_interface.clone()));
                }

                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&auth_path);
                let _ = fs::remove_file(&ovpn_path);
                Err(TunnelError::Elevation(e))
            }
        }
    }

    /// Stops any active tunnel (WireGuard or OpenVPN).
    pub fn stop_tunnel(&self, protocol: VpnProtocol) -> Result<(), TunnelError> {
        match protocol {
            VpnProtocol::WireGuard => {
                validate_interface_name(&self.wg_interface)?;
                let config_path = self.runtime_dir.join(format!("{}.conf", self.wg_interface));
                let target = if config_path.exists() {
                    config_path.to_str().unwrap_or(&self.wg_interface)
                } else {
                    &self.wg_interface
                };

                let res = self.elevation.wg_quick_down(target);
                let _ = fs::remove_file(&config_path);
                res.map(|_| ()).map_err(TunnelError::Elevation)
            }
            VpnProtocol::OpenVpnUdp | VpnProtocol::OpenVpnTcp => {
                validate_interface_name(&self.ovpn_interface)?;
                let ovpn_path = self.runtime_dir.join(format!("{}.ovpn", self.ovpn_interface));
                let pid_path = self.ovpn_pid_path();

                let res = self.elevation.openvpn_down(&self.ovpn_interface, &pid_path);
                let _ = fs::remove_file(self.ovpn_auth_path());
                let _ = fs::remove_file(&ovpn_path);
                let _ = fs::remove_file(&pid_path);
                res.map(|_| ()).map_err(TunnelError::Elevation)
            }
        }
    }

    /// Stops only the tunnels that are actually running, so the user is not asked to
    /// authenticate for a `pkexec` call that has nothing to tear down.
    /// Returns `NotConnected` when nothing was running.
    pub fn stop_active(&self) -> Result<(), TunnelError> {
        let mut stopped_any = false;
        let mut first_err = None;

        if self.is_interface_active(&self.wg_interface) {
            stopped_any = true;
            if let Err(e) = self.stop_tunnel(VpnProtocol::WireGuard) {
                first_err.get_or_insert(e);
            }
        }

        // A daemon may exist without its tun device (e.g. still handshaking).
        if self.is_interface_active(&self.ovpn_interface) || self.ovpn_pid_path().exists() {
            stopped_any = true;
            if let Err(e) = self.stop_tunnel(VpnProtocol::OpenVpnUdp) {
                first_err.get_or_insert(e);
            }
        }

        match (stopped_any, first_err) {
            (false, _) => Err(TunnelError::NotConnected),
            (true, Some(e)) => Err(e),
            (true, None) => Ok(()),
        }
    }

    /// Reads transfer statistics from `/sys/class/net/<interface>/statistics/`.
    pub fn read_statistics(&self, iface: &str) -> Option<NetworkStats> {
        if !self.is_interface_active(iface) {
            return None;
        }

        let stat_dir = Path::new("/sys/class/net").join(iface).join("statistics");
        let rx_file = stat_dir.join("rx_bytes");
        let tx_file = stat_dir.join("tx_bytes");

        let rx_bytes = fs::read_to_string(rx_file)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        let tx_bytes = fs::read_to_string(tx_file)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        Some(NetworkStats {
            rx_bytes,
            tx_bytes,
            rx_rate_bps: 0.0,
            tx_rate_bps: 0.0,
            last_handshake: None,
        })
    }
}
