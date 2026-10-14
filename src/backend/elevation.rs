use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ElevationError {
    #[error("pkexec binary not found at standard paths. Ensure polkit is installed.")]
    PkexecNotFound,
    #[error("wg-quick binary not found. Install wireguard-tools (e.g. 'sudo pacman -S wireguard-tools').")]
    WgQuickNotFound,
    #[error("Elevation was cancelled or dismissed by the user.")]
    AuthenticationCancelled,
    #[error("Authentication failed: invalid credentials or permission denied.")]
    AuthenticationFailed,
    #[error("Invalid interface name '{0}'. Must be 1-15 alphanumeric/underscore/hyphen characters.")]
    InvalidInterfaceName(String),
    #[error("Invalid configuration path '{0}': path traversal or unsafe characters detected.")]
    UnsafePath(String),
    #[error("System I/O error: {0}")]
    IoError(String),
    #[error("Command execution failed ({0}): {1}")]
    ExecutionFailed(i32, String),
}

/// Validates interface name according to Linux network interface name rules (IFNAMSIZ = 16, so max 15 chars).
pub fn validate_interface_name(name: &str) -> Result<(), ElevationError> {
    if name.is_empty() || name.len() > 15 {
        return Err(ElevationError::InvalidInterfaceName(format!(
            "{name} (length must be 1-15 characters)"
        )));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ElevationError::InvalidInterfaceName(format!(
            "{name} (contains invalid characters)"
        )));
    }

    Ok(())
}

/// Checks if a file path is safe to pass to elevated commands.
pub fn validate_safe_path(path: &Path) -> Result<PathBuf, ElevationError> {
    let canonical = path
        .canonicalize()
        .map_err(|e| ElevationError::IoError(format!("Cannot canonicalize path {:?}: {e}", path)))?;

    let path_str = canonical.to_string_lossy();
    if path_str.contains('\0') || path_str.contains('\n') || path_str.contains('\r') {
        return Err(ElevationError::UnsafePath(path_str.to_string()));
    }

    Ok(canonical)
}

/// Safe root elevation manager via PolicyKit (`pkexec`).
#[derive(Debug, Clone)]
pub struct ElevationManager {
    pkexec_path: PathBuf,
}

impl Default for ElevationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ElevationManager {
    pub fn new() -> Self {
        let pkexec_path = which("pkexec").unwrap_or_else(|| PathBuf::from("/usr/bin/pkexec"));
        Self { pkexec_path }
    }

    /// Check if `pkexec` is present and executable.
    pub fn is_pkexec_available(&self) -> bool {
        self.pkexec_path.exists()
    }

    /// Check if `wg-quick` is installed on the system.
    pub fn is_wg_quick_available(&self) -> bool {
        which("wg-quick").is_some() || Path::new("/usr/bin/wg-quick").exists()
    }

    /// Execute `wg-quick up <config_path>` safely via `pkexec`.
    pub fn wg_quick_up(&self, config_path: &Path) -> Result<String, ElevationError> {
        self.ensure_binaries()?;
        let safe_path = validate_safe_path(config_path)?;

        // Execute directly without shell invocation
        let output = self.run_pkexec(&[
            "wg-quick",
            "up",
            safe_path.to_str().ok_or_else(|| {
                ElevationError::UnsafePath("Non-UTF8 path".to_string())
            })?,
        ])?;

        Ok(output)
    }

    /// Execute `wg-quick down <interface_name_or_config_path>` safely via `pkexec`.
    pub fn wg_quick_down(&self, interface_or_path: &str) -> Result<String, ElevationError> {
        self.ensure_binaries()?;

        // If it's a file path
        if interface_or_path.ends_with(".conf") || interface_or_path.contains('/') {
            let path = Path::new(interface_or_path);
            let safe_path = validate_safe_path(path)?;
            return self.run_pkexec(&[
                "wg-quick",
                "down",
                safe_path.to_str().ok_or_else(|| {
                    ElevationError::UnsafePath("Non-UTF8 path".to_string())
                })?,
            ]);
        }

        // Otherwise it's an interface name
        validate_interface_name(interface_or_path)?;
        self.run_pkexec(&["wg-quick", "down", interface_or_path])
    }

    /// Execute a WireGuard diagnostics check via `pkexec wg show <interface>`.
    pub fn wg_show(&self, interface_name: &str) -> Result<String, ElevationError> {
        validate_interface_name(interface_name)?;
        self.run_pkexec(&["wg", "show", interface_name])
    }

    fn ensure_binaries(&self) -> Result<(), ElevationError> {
        if !self.is_pkexec_available() {
            return Err(ElevationError::PkexecNotFound);
        }
        if !self.is_wg_quick_available() {
            return Err(ElevationError::WgQuickNotFound);
        }
        Ok(())
    }

    /// Invokes `pkexec` directly with discrete command arguments (prevents shell injection).
    fn run_pkexec(&self, args: &[&str]) -> Result<String, ElevationError> {
        let mut cmd = Command::new(&self.pkexec_path);
        cmd.args(args);

        // Environment variables for pkexec
        cmd.env("LC_ALL", "C");

        let output: Output = cmd.output().map_err(|e| {
            ElevationError::IoError(format!("Failed to spawn pkexec: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}").trim().to_string()
            };
            return Ok(combined);
        }

        let exit_code = output.status.code().unwrap_or(-1);
        match exit_code {
            126 => Err(ElevationError::AuthenticationCancelled),
            127 => {
                if stderr.contains("not found") {
                    Err(ElevationError::WgQuickNotFound)
                } else {
                    Err(ElevationError::AuthenticationFailed)
                }
            }
            _ => Err(ElevationError::ExecutionFailed(
                exit_code,
                stderr.trim().to_string(),
            )),
        }
    }

    /// Writes configuration to a secure file with strict 0600 permissions.
    pub fn write_secure_config(target_path: &Path, content: &str) -> Result<(), ElevationError> {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ElevationError::IoError(format!("Failed to create directory {:?}: {e}", parent))
            })?;
        }

        fs::write(target_path, content).map_err(|e| {
            ElevationError::IoError(format!("Failed to write config {:?}: {e}", target_path))
        })?;

        // Restrict permissions to owner read/write (0600)
        let mut perms = fs::metadata(target_path)
            .map_err(|e| ElevationError::IoError(e.to_string()))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(target_path, perms).map_err(|e| {
            ElevationError::IoError(format!("Failed to set 0600 permissions: {e}"))
        })?;

        Ok(())
    }

    /// Generates PolicyKit action file content for FastestVPN.
    pub fn generate_polkit_policy() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC
 "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1.0/policyconfig.dtd">
<policyconfig>
  <vendor>FastestVPN Linux Client</vendor>
  <vendor_url>https://fastestvpn.com</vendor_url>
  <action id="com.fastestvpn.wgquick">
    <description>Manage FastestVPN WireGuard Tunnels</description>
    <message>Authentication is required to start or stop FastestVPN WireGuard tunnels</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>yes</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">/usr/bin/wg-quick</annotate>
    <annotate key="org.freedesktop.policykit.exec.allow_gui">true</annotate>
  </action>
</policyconfig>
"#
    }
}

/// Helper to search for a binary in system PATH.
pub fn which(binary_name: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(binary_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // Common standard fallback locations
    for standard_dir in &["/usr/bin", "/usr/sbin", "/bin", "/sbin", "/usr/local/bin"] {
        let candidate = Path::new(standard_dir).join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_interface_name() {
        assert!(validate_interface_name("wg-fastest").is_ok());
        assert!(validate_interface_name("fastestvpn0").is_ok());
        assert!(validate_interface_name("wg_vpn").is_ok());

        assert!(validate_interface_name("").is_err());
        assert!(validate_interface_name("this_is_too_long_for_ifnamsiz_linux").is_err());
        assert!(validate_interface_name("wg;rm -rf /").is_err());
        assert!(validate_interface_name("wg$test").is_err());
        assert!(validate_interface_name("wg test").is_err());
    }

    #[test]
    fn test_polkit_policy_format() {
        let policy = ElevationManager::generate_polkit_policy();
        assert!(policy.contains("com.fastestvpn.wgquick"));
        assert!(policy.contains("/usr/bin/wg-quick"));
    }
}
