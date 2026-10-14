# FastestVPN Linux Desktop Client (Rust + egui + WireGuard)

A modern, lightweight, high-performance Linux desktop client for **FastestVPN** powered by **WireGuard**, built with **Rust** and **egui / eframe**.

---

## 🌟 Key Features

- **Lightweight egui GUI**: Instant startup, minimal memory footprint (~30 MB RAM), responsive dark-mode UI with FastestVPN brand styling.
- **WireGuard `.conf` Profile Engine**:
  - Full support for FastestVPN WireGuard configuration profiles (`[Interface]`, `[Peer]`).
  - Key validation (32-byte Base64), CIDR address validation, endpoint parsing.
  - Native file picker (`rfd`) to import individual `.conf` files or directories.
  - Built-in FastestVPN server presets across North America, Europe, Asia, and Oceania.
  - Account Credentials Manager: input your FastestVPN WireGuard PrivateKey & Assigned IP once, and all built-in server presets become active!
- **Safe Root Elevation via `pkexec`**:
  - Eliminates shell injection vulnerabilities: discrete argument passing via `std::process::Command` without shell (`sh -c`) wrappers.
  - Interface name validation (`IFNAMSIZ` 15-character constraint, alphanumeric + `_` + `-`).
  - Secure profile storage with `0600` (`-rw-------`) Linux file permissions to protect private keys.
  - Handles Polkit authorization gracefully (native desktop password prompt on KDE/GNOME/XFCE/Wayland).
  - Optional PolicyKit policy file (`com.fastestvpn.wgquick.policy`) to allow passwordless tunnel switching.
- **`wg-quick` Tunnel Controller**:
  - Clean lifecycle management (`wg-quick up` / `wg-quick down`).
  - Non-root real-time traffic statistics (`/sys/class/net/wg-fastestvpn/statistics/`).
  - Live download/upload speed meters, total transfer counters, uptime tracking.
  - Stray interface detection and cleanup tool.
- **CLI & Diagnostic Mode**: Can be operated either as a graphical desktop application or via command line flags (`--list`, `--status`, `--down`, `--doctor`).

---

## 🏗 Architecture Overview

```
fastestvpn-gui/
├── src/
│   ├── main.rs                   # Entry point (GUI launcher & CLI flags)
│   ├── lib.rs                    # Library export
│   ├── backend/
│   │   ├── mod.rs                # Public backend API
│   │   ├── config.rs             # WireGuard INI parser, validator & FastestVPN presets
│   │   ├── elevation.rs          # Safe PolicyKit (pkexec) elevation & security checks
│   │   ├── tunnel.rs             # wg-quick lifecycle & sysfs stats reader
│   │   ├── storage.rs            # Profiles & settings persistent storage
│   │   └── service.rs            # Threaded asynchronous background service worker
│   └── gui/
│       ├── mod.rs                # GUI module exports
│       ├── app.rs                # eframe::App event loop and navigation
│       ├── theme.rs              # Custom dark palette, styling, metric formatters
│       └── views/
│           ├── dashboard.rs      # Main connect/disconnect view, speedometers, stats
│           ├── profiles.rs       # Profile manager, search, import & credentials modal
│           ├── diagnostics.rs    # Dependency checks (pkexec, wg-quick, polkit) & fixes
│           └── logs.rs           # Real-time backend & elevation event log viewer
├── tests/
│   └── integration_tests.rs      # Unit and integration test suite
├── com.fastestvpn.wgquick.policy # PolicyKit action rule
└── fastestvpn-gui.desktop        # Linux desktop shortcut
```

---

## 📋 System Prerequisites

1. **WireGuard Tools**:
   ```bash
   # Arch / CachyOS / Manjaro
   sudo pacman -S wireguard-tools

   # Ubuntu / Debian
   sudo apt install wireguard-tools

   # Fedora
   sudo dnf install wireguard-tools
   ```
2. **PolicyKit (pkexec)**:
   Pre-installed on almost all standard Linux desktop installations (KDE Plasma, GNOME, XFCE).

---

## 🚀 Building & Running

### 1. Build from Source
```bash
cargo build --release
```

### 2. Launch Desktop GUI
```bash
cargo run --release
```

### 3. Command Line Helpers
```bash
# Display help and available commands
cargo run -- --help

# List loaded FastestVPN server profiles
cargo run -- --list

# Inspect active WireGuard tunnel status and data transfer
cargo run -- --status

# Disconnect any active tunnel
cargo run -- --down

# Check system prerequisites and paths
cargo run -- --doctor
```

---

## 🔒 Security Model

1. **No Shell Invocations**: Commands are invoked directly via `Command::new("pkexec").args(["wg-quick", ...])`. No shell string concatenation is performed, neutralizing command injection vulnerabilities.
2. **Interface Sanitization**: Tunnel interface names are verified against strict regular expressions: `^[a-zA-Z0-9_-]{1,15}$`.
3. **Strict File Permissions**: Tunnel `.conf` files containing private keys are written with `0600` permissions (`S_IRUSR | S_IWUSR`), ensuring non-owner users cannot read private keys.
4. **Privilege Separation**: The GUI application runs entirely in unprivileged user space. Only the specific `wg-quick` invocation triggers PolicyKit authentication.
