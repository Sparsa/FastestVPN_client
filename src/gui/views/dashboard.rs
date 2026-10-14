use crate::backend::config::{VpnProfile, VpnProtocol};
use crate::gui::views::profiles::protocol_badge;
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::storage::AppSettings;
use crate::backend::tunnel::{NetworkStats, TunnelState};
use crate::gui::theme::{
    card_frame, format_bytes, format_duration, format_rate, COLOR_ACCENT, COLOR_CONNECTED,
    COLOR_CONNECTING, COLOR_ERROR, COLOR_TEXT, COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Align, Button, Color32, Layout, RichText, Sense, Vec2};
use std::time::Instant;

pub struct DashboardView;

/// Requests from the dashboard that the app shell handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardAction {
    OpenServers,
}

impl DashboardView {
    pub fn render(
        ui: &mut egui::Ui,
        service: &VpnBackendService,
        settings: &AppSettings,
        profiles: &[VpnProfile],
        selected_profile_id: &str,
    ) -> Option<DashboardAction> {
        let mut action = None;
        let current_state = service.current_state.lock().unwrap().clone();
        let stats = service.latest_stats.lock().unwrap().clone();

        ui.add_space(10.0);

        // Header status bar
        Self::render_status_banner(ui, &current_state);

        ui.add_space(16.0);

        // Main Connect / Disconnect Action Card
        card_frame().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);

                let is_busy = matches!(current_state, TunnelState::Connected { .. } | TunnelState::Connecting { .. });

                // Selected server summary; choosing happens in the side pane.
                let selected = profiles.iter().find(|p| p.id == selected_profile_id);
                let effective_protocol = selected
                    .map(|p| if p.is_preset { settings.preferred_protocol } else { p.protocol })
                    .unwrap_or(settings.preferred_protocol);

                ui.horizontal(|ui| {
                    let summary_width = 360.0_f32.min(ui.available_width());
                    ui.add_space(((ui.available_width() - summary_width) / 2.0).max(0.0));
                    ui.vertical(|ui| {
                        ui.set_width(summary_width);
                        ui.horizontal(|ui| {
                            match selected {
                                Some(p) => {
                                    ui.label(RichText::new(&p.country_code).strong().size(13.0).color(COLOR_ACCENT));
                                    ui.label(RichText::new(&p.name).strong().size(16.0).color(COLOR_TEXT));
                                }
                                None => {
                                    ui.label(RichText::new("No server selected").size(16.0).color(COLOR_TEXT_MUTED));
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add_enabled(!is_busy, Button::new("Change ›")).clicked() {
                                    action = Some(DashboardAction::OpenServers);
                                }
                            });
                        });
                        let (proto_label, proto_color) = protocol_badge(effective_protocol);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(proto_label).strong().size(12.0).color(proto_color));
                            if let Some(p) = selected {
                                ui.label(RichText::new(format!("• {}", p.display_endpoint())).size(12.0).color(COLOR_TEXT_MUTED));
                            }
                        });
                    });
                });

                ui.add_space(18.0);

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
                    } else if let Some(selected) = selected {
                        let mut target_profile = selected.clone();
                        // The protocol picker applies to built-in presets only; imported
                        // .conf/.ovpn profiles can only be run with their own protocol.
                        if target_profile.is_preset {
                            target_profile.protocol = settings.preferred_protocol;
                        }

                        service.send_command(ServiceCommand::Connect {
                            profile: target_profile,
                            username: settings.username.clone(),
                            password: settings.password.clone(),
                        });
                    }
                }

                ui.add_space(12.0);

                // Status Message Subtitle
                match &current_state {
                    TunnelState::Connected { endpoint, protocol, interface, connected_at, .. } => {
                        let duration = Instant::now().duration_since(*connected_at).as_secs();
                        ui.label(
                            RichText::new(format!(
                                "Interface: {} • Protocol: {} • Endpoint: {} • Uptime: {}",
                                interface,
                                protocol,
                                endpoint,
                                format_duration(duration)
                            ))
                            .color(COLOR_TEXT_MUTED)
                            .size(12.5),
                        );
                    }
                    TunnelState::Disconnected => {
                        if settings.preferred_protocol != VpnProtocol::WireGuard && settings.username.trim().is_empty() {
                            ui.label(
                                RichText::new("⚠️ OpenVPN requires your FastestVPN login. Open ☰ → Account to sign in.")
                                    .color(COLOR_CONNECTING)
                                    .size(12.5),
                            );
                        } else {
                            ui.label(
                                RichText::new("Your internet traffic is unencrypted. Connect to FastestVPN to secure it.")
                                    .color(COLOR_TEXT_MUTED)
                                    .size(12.5),
                            );
                        }
                    }
                    TunnelState::Connecting { server_name, protocol, .. } => {
                        ui.label(
                            RichText::new(format!("Elevating via pkexec and securing {protocol} tunnel to {server_name}..."))
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

                ui.add_space(8.0);
            });
        });

        ui.add_space(16.0);

        // Realtime Network Statistics Cards
        Self::render_stats_grid(ui, &stats, &current_state);

        action
    }

    fn render_status_banner(ui: &mut egui::Ui, state: &TunnelState) {
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot_color, status_text, proto_label) = match state {
                    TunnelState::Connected { profile_name, protocol, .. } => (
                        COLOR_CONNECTED,
                        format!("PROTECTED — CONNECTED TO {}", profile_name.to_uppercase()),
                        format!("{protocol}"),
                    ),
                    TunnelState::Connecting { server_name, protocol, .. } => (
                        COLOR_CONNECTING,
                        format!("ESTABLISHING SECURE TUNNEL TO {}...", server_name.to_uppercase()),
                        format!("{protocol}"),
                    ),
                    TunnelState::Disconnecting => (COLOR_CONNECTING, "DISCONNECTING TUNNEL...".to_string(), "VPN".to_string()),
                    TunnelState::Disconnected => (Color32::from_rgb(156, 163, 175), "UNPROTECTED — DISCONNECTED".to_string(), "Disabled".to_string()),
                    TunnelState::Error { .. } => (COLOR_ERROR, "CONNECTION ERROR".to_string(), "Error".to_string()),
                };

                let (rect, _response) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 6.0, dot_color);

                ui.add_space(8.0);
                ui.label(RichText::new(status_text).strong().size(14.0).color(COLOR_TEXT));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(proto_label).color(COLOR_ACCENT).size(12.5).strong());
                });
            });
        });
    }

    /// Draws the direction arrow directly; the bundled fonts lack the ↓/↑ glyphs.
    fn paint_arrow(ui: &mut egui::Ui, down: bool, color: Color32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(24.0, 28.0), Sense::hover());
        let c = rect.center();
        let (tip, tail) = if down { (c.y + 9.0, c.y - 9.0) } else { (c.y - 9.0, c.y + 9.0) };
        let stroke = egui::Stroke::new(2.5, color);
        let painter = ui.painter();
        painter.line_segment([egui::pos2(c.x, tail), egui::pos2(c.x, tip)], stroke);
        let wing = if down { -7.0 } else { 7.0 };
        painter.line_segment([egui::pos2(c.x - 6.0, tip + wing), egui::pos2(c.x, tip)], stroke);
        painter.line_segment([egui::pos2(c.x + 6.0, tip + wing), egui::pos2(c.x, tip)], stroke);
    }

    fn render_stats_grid(ui: &mut egui::Ui, stats: &NetworkStats, state: &TunnelState) {
        let is_connected = matches!(state, TunnelState::Connected { .. });

        ui.columns(2, |columns| {
            // Download Card
            card_frame().show(&mut columns[0], |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    Self::paint_arrow(ui, true, COLOR_ACCENT);
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
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    Self::paint_arrow(ui, false, COLOR_CONNECTED);
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
