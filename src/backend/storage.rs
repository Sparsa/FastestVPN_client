use crate::backend::config::{
    get_fastestvpn_presets, load_profile_from_file, VpnProfile,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub user_private_key: String,
    pub user_assigned_ip: String,
    pub default_profile_id: Option<String>,
    pub killswitch_enabled: bool,
    pub auto_reconnect: bool,
    pub dark_mode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            user_private_key: String::new(),
            user_assigned_ip: "10.13.1.2/32".to_string(),
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
        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(&self.settings_file, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Load all saved profiles from the config directory.
    pub fn load_all_profiles(&self, settings: &AppSettings) -> Vec<VpnProfile> {
        let mut profiles = Vec::new();

        // 1. Scan profiles directory for .conf files
        if let Ok(entries) = fs::read_dir(&self.profiles_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("conf") {
                    if let Ok(mut profile) = load_profile_from_file(&path) {
                        profile.file_path = Some(path);
                        profiles.push(profile);
                    }
                }
            }
        }

        // 2. Add FastestVPN built-in presets (using user key/ip if provided)
        let presets = get_fastestvpn_presets(&settings.user_private_key, &settings.user_assigned_ip);
        for preset in presets {
            // Only add if not already imported under same ID
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
        let target_filename = format!("{}.conf", profile.id);
        let target_path = self.profiles_dir.join(target_filename);

        let content = profile.config.to_conf_string();
        fs::write(&target_path, content).map_err(|e| e.to_string())?;

        Ok(target_path)
    }

    /// Delete a saved profile file.
    pub fn delete_profile(&self, profile_id: &str) -> Result<(), String> {
        let target_filename = format!("{}.conf", profile_id);
        let target_path = self.profiles_dir.join(target_filename);
        if target_path.exists() {
            fs::remove_file(target_path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Import an existing .conf file by copying it into the profiles directory.
    pub fn import_file(&self, source_path: &Path) -> Result<VpnProfile, String> {
        let mut profile = load_profile_from_file(source_path)
            .map_err(|e| format!("Failed to parse {}: {}", source_path.display(), e))?;

        let saved_path = self.save_profile(&profile)?;
        profile.file_path = Some(saved_path);
        profile.is_preset = false;

        Ok(profile)
    }
}
