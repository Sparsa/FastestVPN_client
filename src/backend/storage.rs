use crate::backend::config::{
    get_fastestvpn_presets, load_profile_from_file, VpnProfile, VpnProtocol,
};
use crate::backend::elevation::ElevationManager;
use crate::backend::openvpn::{load_ovpn_profile_from_file, FASTESTVPN_PORT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub username: String,
    pub password: String,
    pub auth_token: Option<String>,
    pub account_status: Option<String>,
    pub plan_name: Option<String>,
    pub expiry_date: Option<String>,
    pub user_private_key: String,
    pub user_assigned_ip: String,
    pub preferred_protocol: VpnProtocol,
    pub default_profile_id: Option<String>,
    pub killswitch_enabled: bool,
    pub auto_reconnect: bool,
    pub dark_mode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            auth_token: None,
            account_status: None,
            plan_name: None,
            expiry_date: None,
            user_private_key: String::new(),
            user_assigned_ip: "10.13.1.2/32".to_string(),
            preferred_protocol: VpnProtocol::WireGuard,
            default_profile_id: None,
            killswitch_enabled: false,
            auto_reconnect: false,
            dark_mode: true,
        }
    }
}

pub struct ProfileStore {
    _config_dir: PathBuf,
    profiles_dir: PathBuf,
    settings_file: PathBuf,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileStore {
    pub fn new() -> Self {
        let base_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("fastestvpn-gui");
        let profiles_dir = base_dir.join("profiles");
        let settings_file = base_dir.join("settings.json");

        let _ = fs::create_dir_all(&profiles_dir);

        Self {
            _config_dir: base_dir,
            profiles_dir,
            settings_file,
        }
    }

    pub fn profiles_dir(&self) -> &Path {
        &self.profiles_dir
    }

    pub fn load_settings(&self) -> AppSettings {
        if self.settings_file.exists() {
            if let Ok(content) = fs::read_to_string(&self.settings_file) {
                if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                    return settings;
                }
            }
        }
        AppSettings::default()
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        // Settings hold the account password and WireGuard private key: keep them 0600.
        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        ElevationManager::write_secure_config(&self.settings_file, &json).map_err(|e| e.to_string())
    }

    /// Load all saved profiles from the config directory.
    pub fn load_all_profiles(&self, settings: &AppSettings) -> Vec<VpnProfile> {
        let mut profiles = Vec::new();

        // 1. Scan profiles directory for .conf and .ovpn files
        if let Ok(entries) = fs::read_dir(&self.profiles_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if ext == "conf" {
                        if let Ok(mut profile) = load_profile_from_file(&path) {
                            profile.file_path = Some(path);
                            profiles.push(profile);
                        }
                    } else if ext == "ovpn" {
                        if let Ok(mut profile) = load_ovpn_profile_from_file(&path) {
                            profile.file_path = Some(path);
                            profiles.push(profile);
                        }
                    }
                }
            }
        }

        // 2. Add FastestVPN built-in presets (using user key/ip if provided)
        let presets = get_fastestvpn_presets(&settings.user_private_key, &settings.user_assigned_ip);
        for mut preset in presets {
            preset.protocol = settings.preferred_protocol;
            if preset.protocol != VpnProtocol::WireGuard {
                // Presets carry WireGuard endpoints; OpenVPN uses the same host on its own port.
                let host = preset.config.peer.endpoint.split(':').next().unwrap_or_default();
                preset.ovpn_endpoint = Some(format!("{host}:{FASTESTVPN_PORT}"));
            }
            if !profiles.iter().any(|p| p.id == preset.id) {
                profiles.push(preset);
            }
        }

        // Sort: User custom/imported profiles first, then alphabetically by country
        profiles.sort_by(|a, b| {
            if a.is_preset != b.is_preset {
                a.is_preset.cmp(&b.is_preset)
            } else {
                a.country.cmp(&b.country).then_with(|| a.city.cmp(&b.city))
            }
        });

        profiles
    }

    /// Save an imported or created profile.
    pub fn save_profile(&self, profile: &VpnProfile) -> Result<PathBuf, String> {
        // Only WireGuard profiles carry their full config in memory; OpenVPN profiles
        // are persisted by copying the original .ovpn in `import_file`.
        if profile.protocol != VpnProtocol::WireGuard {
            return Err("Only WireGuard profiles can be saved from the editor; import .ovpn files instead".to_string());
        }
        let target_path = self.profiles_dir.join(format!("{}.conf", profile.id));

        let content = profile.config.to_conf_string();
        ElevationManager::write_secure_config(&target_path, &content).map_err(|e| e.to_string())?;

        Ok(target_path)
    }

    /// Delete a saved profile file.
    pub fn delete_profile(&self, profile_id: &str) -> Result<(), String> {
        for ext in &["conf", "ovpn"] {
            let target_path = self.profiles_dir.join(format!("{}.{}", profile_id, ext));
            if target_path.exists() {
                let _ = fs::remove_file(target_path);
            }
        }
        Ok(())
    }

    /// Import an existing .conf or .ovpn file by copying it into the profiles directory.
    pub fn import_file(&self, source_path: &Path) -> Result<VpnProfile, String> {
        let ext = source_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let mut profile = if ext == "ovpn" {
            load_ovpn_profile_from_file(source_path)
                .map_err(|e| format!("Failed to parse {}: {}", source_path.display(), e))?
        } else {
            load_profile_from_file(source_path)
                .map_err(|e| format!("Failed to parse {}: {}", source_path.display(), e))?
        };

        let target_filename = format!("{}.{}", profile.id, ext);
        let target_path = self.profiles_dir.join(target_filename);
        fs::copy(source_path, &target_path)
            .map_err(|e| format!("Failed to copy {} into profile store: {e}", source_path.display()))?;
        let _ = fs::set_permissions(&target_path, fs::Permissions::from_mode(0o600));

        profile.file_path = Some(target_path);
        profile.is_preset = false;

        Ok(profile)
    }
}
