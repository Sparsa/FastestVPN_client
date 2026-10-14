use crate::backend::config::VpnProfile;
use crate::backend::elevation::{validate_interface_name, ElevationError, ElevationManager};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

pub const DEFAULT_INTERFACE_NAME: &str = "wg-fastestvpn";

#[derive(Error, Debug, Clone)]
pub enum TunnelError {
    #[error("Elevation failed: {0}")]
    Elevation(#[from] ElevationError),
    #[error("Tunnel already active on interface {0}")]
    AlreadyConnected(String),
    #[error("No active tunnel found to disconnect")]
    NotConnected,
    #[error("I/O error during tunnel operation: {0}")]
    Io(String),
    #[error("Tunnel verification failed: interface {0} did not come up")]
    InterfaceVerificationFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting { server_name: String, started_at: Instant },
    Connected {
        profile_name: String,
        endpoint: String,
        interface: String,
        connected_at: Instant,
    },
    Disconnecting,
    Error { message: String },
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
    interface_name: String,
    runtime_dir: PathBuf,
}

impl Default for TunnelController {
    fn default() -> Self {
        Self::new(DEFAULT_INTERFACE_NAME)
    }
}

impl TunnelController {
    pub fn new(interface_name: &str) -> Self {
        let elevation = ElevationManager::new();
        let runtime_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("fastestvpn-gui")
            .join("runtime");

        Self {
            elevation,
            interface_name: interface_name.to_string(),
            runtime_dir,
        }
    }

    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }

    pub fn elevation_manager(&self) -> &ElevationManager {
        &self.elevation
    }

    /// Path to the active runtime config file: e.g. `~/.config/fastestvpn-gui/runtime/wg-fastestvpn.conf`
    pub fn runtime_config_path(&self) -> PathBuf {
        self.runtime_dir.join(format!("{}.conf", self.interface_name))
    }

    /// Check if the network interface exists in the Linux sysfs (e.g. `/sys/class/net/wg-fastestvpn`).
    pub fn is_interface_active(&self) -> bool {
        let sys_path = Path::new("/sys/class/net").join(&self.interface_name);
        sys_path.exists()
    }

    /// Starts a WireGuard tunnel for the specified profile.
    pub fn start_tunnel(&self, profile: &VpnProfile) -> Result<(), TunnelError> {
        validate_interface_name(&self.interface_name)?;

        if self.is_interface_active() {
            return Err(TunnelError::AlreadyConnected(self.interface_name.clone()));
        }

        // Prepare runtime directory
        fs::create_dir_all(&self.runtime_dir)
            .map_err(|e| TunnelError::Io(format!("Failed to create runtime dir: {e}")))?;

        let config_path = self.runtime_config_path();
        let conf_content = profile.config.to_conf_string();

        // Write configuration securely with 0600 permissions
        ElevationManager::write_secure_config(&config_path, &conf_content)?;

        // Bring up tunnel via pkexec wg-quick
        match self.elevation.wg_quick_up(&config_path) {
            Ok(_) => {
                // Verify interface appeared
                if !self.is_interface_active() {
                    // Give a small grace period for network device registration
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }

                if !self.is_interface_active() {
                    // Try to clean up if interface failed to appear
                    let _ = self.elevation.wg_quick_down(config_path.to_str().unwrap_or(""));
                    return Err(TunnelError::InterfaceVerificationFailed(self.interface_name.clone()));
                }

                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&config_path);
                Err(TunnelError::Elevation(e))
            }
        }
    }

    /// Stops the active WireGuard tunnel.
    pub fn stop_tunnel(&self) -> Result<(), TunnelError> {
        validate_interface_name(&self.interface_name)?;

        let config_path = self.runtime_config_path();

        let target = if config_path.exists() {
            config_path.to_str().unwrap_or(&self.interface_name)
        } else {
            &self.interface_name
        };

        // Try tearing down
        let result = self.elevation.wg_quick_down(target);

        // Clean up runtime config file
        if config_path.exists() {
            let _ = fs::remove_file(&config_path);
        }

        match result {
            Ok(_) => Ok(()),
            Err(ElevationError::ExecutionFailed(_, ref msg)) if msg.contains("is not a WireGuard interface") => {
                // If it wasn't up, ignore
                Ok(())
            }
            Err(e) => Err(TunnelError::Elevation(e)),
        }
    }

    /// Read tunnel transfer statistics directly from `/sys/class/net/<interface>/statistics/` (no root required).
    pub fn read_statistics(&self) -> Option<NetworkStats> {
        if !self.is_interface_active() {
            return None;
        }

        let stat_dir = Path::new("/sys/class/net").join(&self.interface_name).join("statistics");
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
