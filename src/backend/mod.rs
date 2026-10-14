pub mod api;
pub mod config;
pub mod elevation;
pub mod openvpn;
pub mod service;
pub mod storage;
pub mod tunnel;

pub use api::{FastestVpnApiClient, UserData};
pub use config::{
    get_fastestvpn_presets, infer_location, load_profile_from_file, parse_wireguard_conf,
    ConfigError, InterfaceConfig, PeerConfig, VpnProfile, VpnProtocol, WireGuardConfig,
};
pub use elevation::{validate_interface_name, ElevationError, ElevationManager};
pub use openvpn::{generate_fastestvpn_ovpn, load_ovpn_profile_from_file};
pub use service::{LogEntry, LogLevel, ServiceCommand, ServiceEvent, VpnBackendService};
pub use storage::{AppSettings, ProfileStore};
pub use tunnel::{
    NetworkStats, TunnelController, TunnelError, TunnelState, DEFAULT_OPENVPN_INTERFACE,
    DEFAULT_WG_INTERFACE,
};
