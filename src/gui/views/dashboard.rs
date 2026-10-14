use crate::backend::config::VpnProfile;
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::tunnel::{NetworkStats, TunnelState};
use crate::gui::theme::{
    card_frame, format_bytes, format_duration, format_rate, COLOR_ACCENT, COLOR_CONNECTED,
    COLOR_CONNECTING, COLOR_ERROR, COLOR_TEXT, COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Align, Button, Color32, Layout, RichText, Sense, Vec2};
use std::time::Instant;

pub struct DashboardView;

impl DashboardView {
    pub fn render(
        ui: &mut egui::Ui,
        service: &VpnBackendService,
        profiles: &[VpnProfile],
        selected_profile_id: &mut String,
    ) {
        let current_state = service.current_state.lock().unwrap().clone();
        let stats = service.latest_stats.lock().unwrap().clone();

        ui.add_space(10.0);

        // Header status bar
        Self::render_status_banner(ui, &current_state);

        ui.add_space(18.0);

        // Main Connect / Disconnect Action Card
        card_frame().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                // Profile Selector Dropdown
                ui.horizontal(|ui| {
                    ui.label(RichText::new("VPN Server:").strong().color(COLOR_TEXT_MUTED));

                    let current_name = profiles
                        .iter()
                        .find(|p| &p.id == selected_profile_id)
                        .map(|p| format!("{} ({})", p.name, p.country_code))
                        .unwrap_or_else(|| "Select a Server Profile".to_string());

                    let is_connected = matches!(current_state, TunnelState::Connected { .. } | TunnelState::Connecting { .. });

                    ui.add_enabled_ui(!is_connected, |ui| {
                        egui::ComboBox::from_id_salt("profile_selector")
                            .selected_text(RichText::new(current_name).color(COLOR_TEXT).strong())
                            .width(320.0)
                            .show_ui(ui, |ui| {
                                for profile in profiles {
                                    let label = format!("[{}] {} - {}", profile.country_code, profile.name, profile.display_endpoint());
                                    ui.selectable_value(
                                        selected_profile_id,
                                        profile.id.clone(),
                                        label,
                                    );
                                }
                            });
                    });
                });

                ui.add_space(20.0);

                // Big Connect / Disconnect Toggle Button
                let is_connected = matches!(current_state, TunnelState::Connected { .. });
                let is_connecting = matches!(current_state, TunnelState::Connecting { .. });
                let is_disconnecting = matches!(current_state, TunnelState::Disconnecting);

                let (btn_text, btn_color) = if is_connected {
                    ("DISCONNECT", Color32::from_rgb(220, 38, 38)) // Red
                } else if is_connecting {
                    ("CONNECTING...", COLOR_CONNECTING)
                } else if is_disconnecting {
                    ("DISCONNECTING...", COLOR_CONNECTING)
                } else {
                    ("CONNECT NOW", COLOR_ACCENT) // Cyan
                };

                let button = Button::new(
                    RichText::new(btn_text)
                        .size(17.0)
                        .strong()
                        .color(Color32::WHITE),
                )
                .min_size(Vec2::new(260.0, 50.0))
                .fill(btn_color);

                let btn_resp = ui.add_enabled(!is_connecting && !is_disconnecting, button);
                if btn_resp.clicked() {
                    if is_connected {
                        service.send_command(ServiceCommand::Disconnect);
                    } else if let Some(selected) = profiles.iter().find(|p| &p.id == selected_profile_id) {
                        service.send_command(ServiceCommand::Connect(selected.clone()));
                    }
                }

                ui.add_space(12.0);

                match &current_state {
                    TunnelState::Connected { endpoint, interface, connected_at, .. } => {
                        let duration = Instant::now().duration_since(*connected_at).as_secs();
                        ui.label(
                            RichText::new(format!(
                                "Interface: {} • Endpoint: {} • Uptime: {}",
                                interface,
                                endpoint,
                                format_duration(duration)
                            ))
                            .color(COLOR_TEXT_MUTED)
                            .size(12.5),
                        );
                    }
                    TunnelState::Disconnected => {
                        ui.label(
                            RichText::new("Your internet traffic is unencrypted. Connect to FastestVPN to secure it.")
                                .color(COLOR_TEXT_MUTED)
                                .size(12.5),
                        );
                    }
                    TunnelState::Connecting { server_name, .. } => {
                        ui.label(
                            RichText::new(format!("Elevating privileges via pkexec and securing tunnel to {}...", server_name))
                                .color(COLOR_CONNECTING)
                                .size(12.5),
                        );
                    }
                    TunnelState::Error { message } => {
                        ui.label(
                            RichText::new(format!("Error: {message}"))
                                .color(COLOR_ERROR)
                                .size(12.5),
                        );
                    }
                    _ => {}
                }

                ui.add_space(10.0);
            });
        });

        ui.add_space(16.0);

        // Realtime Network Statistics Cards
        Self::render_stats_grid(ui, &stats, &current_state);
    }

    fn render_status_banner(ui: &mut egui::Ui, state: &TunnelState) {
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot_color, status_text) = match state {
                    TunnelState::Connected { profile_name, .. } => {
                        (COLOR_CONNECTED, format!("PROTECTED — CONNECTED TO {}", profile_name.to_uppercase()))
                    }
                    TunnelState::Connecting { server_name, .. } => {
                        (COLOR_CONNECTING, format!("ESTABLISHING SECURE TUNNEL TO {}...", server_name.to_uppercase()))
                    }
                    TunnelState::Disconnecting => (COLOR_CONNECTING, "DISCONNECTING TUNNEL...".to_string()),
                    TunnelState::Disconnected => (Color32::from_rgb(156, 163, 175), "UNPROTECTED — DISCONNECTED".to_string()),
                    TunnelState::Error { .. } => (COLOR_ERROR, "CONNECTION ERROR".to_string()),
                };

                let (rect, _response) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 6.0, dot_color);

                ui.add_space(8.0);
                ui.label(RichText::new(status_text).strong().size(14.0).color(COLOR_TEXT));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("WireGuard Protocol").color(COLOR_TEXT_MUTED).size(12.0));
                });
            });
        });
    }

    fn render_stats_grid(ui: &mut egui::Ui, stats: &NetworkStats, state: &TunnelState) {
        let is_connected = matches!(state, TunnelState::Connected { .. });

        ui.columns(2, |columns| {
            // Download Card
            card_frame().show(&mut columns[0], |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("↓").size(24.0).color(COLOR_ACCENT).strong());
                    ui.vertical(|ui| {
                        ui.label(RichText::new("DOWNLOAD SPEED").size(11.0).color(COLOR_TEXT_MUTED).strong());
                        let speed_str = if is_connected {
                            format_rate(stats.rx_rate_bps)
                        } else {
                            "0.0 KB/s".to_string()
                        };
                        ui.label(RichText::new(speed_str).size(20.0).strong().color(COLOR_TEXT));

                        let total_str = if is_connected {
                            format_bytes(stats.rx_bytes)
                        } else {
                            "0 B".to_string()
                        };
                        ui.label(RichText::new(format!("Total: {total_str}")).size(12.0).color(COLOR_TEXT_MUTED));
                    });
                });
            });

            // Upload Card
            card_frame().show(&mut columns[1], |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("↑").size(24.0).color(COLOR_CONNECTED).strong());
                    ui.vertical(|ui| {
                        ui.label(RichText::new("UPLOAD SPEED").size(11.0).color(COLOR_TEXT_MUTED).strong());
                        let speed_str = if is_connected {
                            format_rate(stats.tx_rate_bps)
                        } else {
                            "0.0 KB/s".to_string()
                        };
                        ui.label(RichText::new(speed_str).size(20.0).strong().color(COLOR_TEXT));

                        let total_str = if is_connected {
                            format_bytes(stats.tx_bytes)
                        } else {
                            "0 B".to_string()
                        };
                        ui.label(RichText::new(format!("Total: {total_str}")).size(12.0).color(COLOR_TEXT_MUTED));
                    });
                });
            });
        });
    }
}
