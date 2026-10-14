use fastestvpn_gui::backend::elevation::which;
use fastestvpn_gui::backend::storage::ProfileStore;
use fastestvpn_gui::backend::tunnel::{TunnelController, DEFAULT_INTERFACE_NAME};
use fastestvpn_gui::gui::FastestVpnApp;
use std::env;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();

    // Support optional quick CLI commands
    if args.len() > 1 {
        match args[1].as_str() {
            "--help" | "-h" => {
                println!("FastestVPN Linux Desktop & CLI Client");
                println!();
                println!("Usage:");
                println!("  fastestvpn-gui              Launch GUI desktop application");
                println!("  fastestvpn-gui --list       List available FastestVPN server profiles");
                println!("  fastestvpn-gui --status     Show active WireGuard tunnel status");
                println!("  fastestvpn-gui --down       Disconnect active tunnel via pkexec");
                println!("  fastestvpn-gui --doctor     Run system & dependency diagnostics");
                return Ok(());
            }
            "--list" | "-l" => {
                let store = ProfileStore::new();
                let settings = store.load_settings();
                let profiles = store.load_all_profiles(&settings);
                println!("Available FastestVPN Profiles ({}):", profiles.len());
                for p in profiles {
                    println!("  • [{}] {:<24} -> {}", p.country_code, p.name, p.display_endpoint());
                }
                return Ok(());
            }
            "--status" | "-s" => {
                let controller = TunnelController::default();
                if controller.is_interface_active() {
                    println!("Tunnel '{}' is ACTIVE.", controller.interface_name());
                    if let Some(stats) = controller.read_statistics() {
                        println!("  Transferred: RX {} bytes, TX {} bytes", stats.rx_bytes, stats.tx_bytes);
                    }
                } else {
                    println!("Tunnel is DISCONNECTED.");
                }
                return Ok(());
            }
            "--down" | "-d" => {
                println!("Disconnecting WireGuard tunnel...");
                let controller = TunnelController::default();
                match controller.stop_tunnel() {
                    Ok(()) => println!("Tunnel disconnected successfully."),
                    Err(e) => eprintln!("Error disconnecting tunnel: {e}"),
                }
                return Ok(());
            }
            "--doctor" => {
                println!("FastestVPN System Diagnostics:");
                println!("  pkexec:       {:?}", which("pkexec"));
                println!("  wg-quick:     {:?}", which("wg-quick"));
                println!("  wg:           {:?}", which("wg"));
                println!("  Interface:    /sys/class/net/{}", DEFAULT_INTERFACE_NAME);
                let polkit_path = std::path::Path::new("/usr/share/polkit-1/actions/com.fastestvpn.wgquick.policy");
                println!("  Polkit Rule:  {}", if polkit_path.exists() { "Installed" } else { "Not installed" });
                return Ok(());
            }
            other => {
                eprintln!("Unknown argument: {other}. Use --help for usage.");
                return Ok(());
            }
        }
    }

    // Launch GUI Desktop Application
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([780.0, 580.0])
            .with_min_inner_size([620.0, 460.0])
            .with_title("FastestVPN Client - WireGuard"),
        ..Default::default()
    };

    eframe::run_native(
        "FastestVPN Client",
        native_options,
        Box::new(|cc| Ok(Box::new(FastestVpnApp::new(cc)))),
    )
}
