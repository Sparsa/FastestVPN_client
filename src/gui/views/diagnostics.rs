use crate::backend::elevation::which;
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::tunnel::TunnelController;
use crate::gui::theme::{
    card_frame, COLOR_ACCENT, COLOR_CONNECTED, COLOR_ERROR, COLOR_TEXT, COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::path::Path;

pub struct DiagnosticsView {
    elevation_test_status: Option<String>,
}

impl Default for DiagnosticsView {
    fn default() -> Self {
        Self {
            elevation_test_status: None,
        }
    }
}

impl DiagnosticsView {
    pub fn render(&mut self, ui: &mut egui::Ui, service: &VpnBackendService) {
        let pkexec_path = which("pkexec");
        let wg_quick_path = which("wg-quick");
        let openvpn_path = which("openvpn");
        let controller = TunnelController::default();
        let active_tunnel = controller.active_interface();
        let policy_path = Path::new("/usr/share/polkit-1/actions/com.fastestvpn.wgquick.policy");
        let policy_installed = policy_path.exists();

        ui.add_space(10.0);

        card_frame().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("🛠 System & VPN Diagnostics").strong().size(15.0).color(COLOR_TEXT));
                ui.label(
                    RichText::new("Checks Linux system components required for safe root elevation, WireGuard, and OpenVPN networking.")
                        .size(12.0)
                        .color(COLOR_TEXT_MUTED),
                );

                ui.add_space(12.0);

                // Check pkexec
                Self::render_check_row(
                    ui,
                    "PolicyKit pkexec",
                    pkexec_path.is_some(),
                    pkexec_path
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not found! Install 'polkit' package.".to_string()),
                );

                // Check wg-quick
                Self::render_check_row(
                    ui,
                    "WireGuard Tools (wg-quick)",
                    wg_quick_path.is_some(),
                    wg_quick_path
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| {
                            "Not installed! Run 'sudo pacman -S wireguard-tools' or 'sudo apt install wireguard-tools'.".to_string()
                        }),
                );

                // Check OpenVPN
                Self::render_check_row(
                    ui,
                    "OpenVPN CLI",
                    openvpn_path.is_some(),
                    openvpn_path
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not installed! Run 'sudo pacman -S openvpn'.".to_string()),
                );

                // Check Active Interface
                Self::render_check_row(
                    ui,
                    "Active VPN Interface",
                    true,
                    if let Some((iface, proto)) = active_tunnel {
                        format!("Active: {iface} ({proto})")
                    } else {
                        "Down / Inactive".to_string()
                    },
                );

                // Check Polkit Policy
                Self::render_check_row(
                    ui,
                    "FastestVPN Polkit Policy",
                    policy_installed,
                    if policy_installed {
                        "Installed (/usr/share/polkit-1/actions/com.fastestvpn.wgquick.policy)".to_string()
                    } else {
                        "Not installed. Installing allows GUI elevation without repetitive password prompts.".to_string()
                    },
                );
            });
        });

        ui.add_space(14.0);

        // Action tools card
        card_frame().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("⚡ Quick Remediation & Maintenance").strong().size(14.0).color(COLOR_TEXT));

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button(RichText::new("🔐 Install Polkit Policy").color(Color32::WHITE)).clicked() {
                        service.send_command(ServiceCommand::InstallPolkitPolicy);
                    }

                    if ui.button(RichText::new("🧹 Tear Down Leftover Tunnel").color(COLOR_ERROR)).clicked() {
                        service.send_command(ServiceCommand::Disconnect);
                    }

                    if ui.button(RichText::new("🔄 Re-check Interfaces")).clicked() {
                        service.send_command(ServiceCommand::CheckInterface);
                    }
                });

                if let Some(ref status) = self.elevation_test_status {
                    ui.add_space(8.0);
                    ui.label(RichText::new(status).color(COLOR_ACCENT).size(12.0));
                }
            });
        });
    }

    fn render_check_row(ui: &mut egui::Ui, name: &str, is_ok: bool, detail: String) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::hover());
            let color = if is_ok { COLOR_CONNECTED } else { COLOR_ERROR };
            ui.painter().circle_filled(rect.center(), 5.0, color);

            ui.add_space(6.0);
            ui.label(RichText::new(name).strong().color(COLOR_TEXT).size(13.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(detail).color(COLOR_TEXT_MUTED).size(12.0));
            });
        });
        ui.add_space(4.0);
    }
}
