use crate::backend::config::{InterfaceConfig, PeerConfig, VpnProfile, VpnProtocol, WireGuardConfig};
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::storage::{AppSettings, ProfileStore};
use crate::backend::tunnel::TunnelState;
use crate::gui::theme::{
    card_frame, COLOR_ACCENT, COLOR_CONNECTED, COLOR_ERROR, COLOR_SURFACE_LIGHT, COLOR_TEXT,
    COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Button, Color32, RichText, ScrollArea, TextEdit, Vec2};

/// Server list, protocol choice and profile import, shown in the side pane.
pub struct ProfilesView {
    search_query: String,
    show_new_dialog: bool,
    import_error: Option<String>,

    // New profile form state
    new_name: String,
    new_private_key: String,
    new_address: String,
    new_dns: String,
    new_public_key: String,
    new_endpoint: String,
    new_form_error: Option<String>,
}

impl Default for ProfilesView {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            show_new_dialog: false,
            import_error: None,
            new_name: String::new(),
            new_private_key: String::new(),
            new_address: "10.13.1.2/32".to_string(),
            new_dns: "1.1.1.1, 1.0.0.1".to_string(),
            new_public_key: String::new(),
            new_endpoint: String::new(),
            new_form_error: None,
        }
    }
}

pub fn protocol_badge(protocol: VpnProtocol) -> (&'static str, Color32) {
    match protocol {
        VpnProtocol::WireGuard => ("WireGuard", COLOR_ACCENT),
        VpnProtocol::OpenVpnUdp => ("OpenVPN UDP", Color32::from_rgb(168, 85, 247)),
        VpnProtocol::OpenVpnTcp => ("OpenVPN TCP", Color32::from_rgb(234, 179, 8)),
    }
}

impl ProfilesView {
    /// Returns `true` when the user started a connection, so the pane can close.
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        service: &VpnBackendService,
        store: &ProfileStore,
        settings: &mut AppSettings,
        profiles: &mut Vec<VpnProfile>,
        selected_profile_id: &mut String,
    ) -> bool {
        let current_state = service.current_state.lock().unwrap().clone();
        let is_busy = matches!(
            current_state,
            TunnelState::Connected { .. } | TunnelState::Connecting { .. } | TunnelState::Disconnecting
        );
        let mut connect_requested = false;

        // Protocol used for the built-in servers
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Protocol").strong().color(COLOR_TEXT_MUTED).size(12.0));
            ui.add_enabled_ui(!is_busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (proto, label) in [
                        (VpnProtocol::WireGuard, "⚡ WireGuard"),
                        (VpnProtocol::OpenVpnUdp, "🛡 OpenVPN UDP"),
                        (VpnProtocol::OpenVpnTcp, "🔒 OpenVPN TCP"),
                    ] {
                        ui.selectable_value(&mut settings.preferred_protocol, proto, label);
                    }
                });
            });
            let hint = if settings.preferred_protocol != VpnProtocol::WireGuard && settings.username.trim().is_empty() {
                RichText::new("⚠ OpenVPN needs your FastestVPN login (Account tab).").color(Color32::from_rgb(234, 179, 8))
            } else {
                RichText::new("Applies to built-in servers; imported profiles keep their own.").color(COLOR_TEXT_MUTED)
            };
            ui.label(hint.size(11.0));
        });

        ui.add_space(8.0);

        // Search + actions
        ui.add(
            TextEdit::singleline(&mut self.search_query)
                .hint_text("🔍 Search country, city, server…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("📁 Import .conf / .ovpn").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("VPN Profiles", &["conf", "ovpn"])
                    .pick_file()
                {
                    match store.import_file(&path) {
                        Ok(imported) => {
                            *selected_profile_id = imported.id.clone();
                            *profiles = store.load_all_profiles(settings);
                            self.import_error = None;
                        }
                        Err(err) => self.import_error = Some(err),
                    }
                }
            }
            if ui.button("➕ New WireGuard Profile").clicked() {
                self.show_new_dialog = !self.show_new_dialog;
                self.new_form_error = None;
            }
        });

        if let Some(ref err) = self.import_error {
            ui.label(RichText::new(err).color(COLOR_ERROR).size(12.0));
        }

        ui.add_space(8.0);

        if self.show_new_dialog {
            self.render_new_profile_dialog(ui, store, settings, profiles, selected_profile_id);
        }

        let query = self.search_query.trim().to_lowercase();
        let filtered_profiles: Vec<VpnProfile> = profiles
            .iter()
            .filter(|p| {
                query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.country.to_lowercase().contains(&query)
                    || p.city.to_lowercase().contains(&query)
                    || p.country_code.to_lowercase().contains(&query)
                    || p.display_endpoint().to_lowercase().contains(&query)
            })
            .cloned()
            .collect();

        ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if filtered_profiles.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No servers match your search.").color(COLOR_TEXT_MUTED));
                });
                return;
            }

            for profile in filtered_profiles {
                let is_selected = &profile.id == selected_profile_id;
                let is_active_tunnel = matches!(
                    &current_state,
                    TunnelState::Connected { profile_name, .. } if profile_name == &profile.name
                );

                let stroke_color = if is_active_tunnel {
                    COLOR_CONNECTED
                } else if is_selected {
                    COLOR_ACCENT
                } else {
                    Color32::from_rgb(51, 65, 85)
                };

                // A sensing container registers before its children, so the buttons
                // inside still take their own clicks; elsewhere the card selects.
                let card = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                    egui::Frame::new()
                        .fill(if is_selected { COLOR_SURFACE_LIGHT } else { Color32::from_rgb(24, 33, 47) })
                        .corner_radius(egui::CornerRadius::same(8))
                        .stroke(egui::Stroke::new(1.2, stroke_color))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                // Country Badge
                                let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), egui::Sense::hover());
                                ui.painter().rect_filled(badge_rect, 4.0, Color32::from_rgb(15, 23, 42));
                                ui.painter().text(
                                    badge_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &profile.country_code,
                                    egui::FontId::proportional(11.5),
                                    COLOR_ACCENT,
                                );

                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&profile.name).strong().size(13.5).color(COLOR_TEXT));
                                    let (proto_label, proto_color) = protocol_badge(profile.protocol);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(proto_label).size(10.5).color(proto_color).strong());
                                        ui.label(
                                            RichText::new(if profile.is_preset { "Preset" } else { "Custom" })
                                                .size(10.5)
                                                .color(if profile.is_preset { COLOR_TEXT_MUTED } else { COLOR_CONNECTED }),
                                        );
                                    });
                                    ui.label(RichText::new(profile.display_endpoint()).size(11.0).color(COLOR_TEXT_MUTED));
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if is_active_tunnel {
                                        let btn = Button::new(RichText::new("Disconnect").size(12.0).strong().color(Color32::WHITE))
                                            .fill(Color32::from_rgb(220, 38, 38));
                                        if ui.add(btn).clicked() {
                                            service.send_command(ServiceCommand::Disconnect);
                                        }
                                    } else {
                                        let btn = Button::new(RichText::new("Connect").size(12.0).strong().color(Color32::WHITE))
                                            .fill(if is_selected { COLOR_ACCENT } else { Color32::from_rgb(51, 65, 85) });
                                        if ui.add_enabled(!is_busy, btn).clicked() {
                                            *selected_profile_id = profile.id.clone();
                                            service.send_command(ServiceCommand::Connect {
                                                profile: profile.clone(),
                                                username: settings.username.clone(),
                                                password: settings.password.clone(),
                                            });
                                            connect_requested = true;
                                        }
                                    }

                                    if !profile.is_preset
                                        && !is_active_tunnel
                                        && ui.button(RichText::new("🗑").size(12.0).color(COLOR_ERROR)).on_hover_text("Delete profile").clicked()
                                    {
                                        let _ = store.delete_profile(&profile.id);
                                        *profiles = store.load_all_profiles(settings);
                                        if selected_profile_id == &profile.id {
                                            if let Some(first) = profiles.first() {
                                                *selected_profile_id = first.id.clone();
                                            }
                                        }
                                    }
                                });
                            });
                        });
                });

                if card.response.clicked() && !is_busy {
                    *selected_profile_id = profile.id.clone();
                }
                card.response.on_hover_cursor(egui::CursorIcon::PointingHand);

                ui.add_space(5.0);
            }
        });

        connect_requested
    }

    fn render_new_profile_dialog(
        &mut self,
        ui: &mut egui::Ui,
        store: &ProfileStore,
        settings: &AppSettings,
        profiles: &mut Vec<VpnProfile>,
        selected_profile_id: &mut String,
    ) {
        let field_width = ui.available_width() - 28.0;
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("➕ New WireGuard Profile").strong().size(14.0).color(COLOR_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        self.show_new_dialog = false;
                    }
                });
            });

            ui.add_space(6.0);
            ui.label("Profile Name");
            ui.add(TextEdit::singleline(&mut self.new_name).hint_text("e.g. Germany Dedicated").desired_width(field_width));
            ui.label("Endpoint (host:port)");
            ui.add(TextEdit::singleline(&mut self.new_endpoint).hint_text("de-01.jumptoserver.com:51820").desired_width(field_width));
            ui.label("Server PublicKey");
            ui.add(TextEdit::singleline(&mut self.new_public_key).hint_text("44-char base64 server key").desired_width(field_width));
            ui.label("Client PrivateKey");
            ui.add(
                TextEdit::singleline(&mut self.new_private_key)
                    .password(true)
                    .hint_text("44-char base64 private key")
                    .desired_width(field_width),
            );
            ui.label("Address");
            ui.add(TextEdit::singleline(&mut self.new_address).hint_text("10.13.1.2/32").desired_width(field_width));
            ui.label("DNS");
            ui.add(TextEdit::singleline(&mut self.new_dns).hint_text("1.1.1.1, 1.0.0.1").desired_width(field_width));

            if let Some(ref err) = self.new_form_error {
                ui.label(RichText::new(format!("Error: {err}")).color(COLOR_ERROR).size(12.0));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save Profile").strong().color(Color32::WHITE)).clicked() {
                    let config = WireGuardConfig {
                        interface: InterfaceConfig {
                            private_key: self.new_private_key.trim().to_string(),
                            address: self.new_address.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                            dns: self.new_dns.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                            mtu: Some(1420),
                            listen_port: None,
                            pre_up: Vec::new(),
                            post_up: Vec::new(),
                            pre_down: Vec::new(),
                            post_down: Vec::new(),
                        },
                        peer: PeerConfig {
                            public_key: self.new_public_key.trim().to_string(),
                            preshared_key: None,
                            endpoint: self.new_endpoint.trim().to_string(),
                            allowed_ips: vec!["0.0.0.0/0".to_string(), "::/0".to_string()],
                            persistent_keepalive: Some(25),
                        },
                    };

                    match config.validate() {
                        Ok(()) => {
                            let profile = VpnProfile::new_custom(
                                if self.new_name.trim().is_empty() { "Custom FastestVPN".to_string() } else { self.new_name.trim().to_string() },
                                config,
                            );
                            match store.save_profile(&profile) {
                                Ok(_) => {
                                    *selected_profile_id = profile.id.clone();
                                    *profiles = store.load_all_profiles(settings);
                                    self.show_new_dialog = false;
                                    self.new_form_error = None;
                                }
                                Err(e) => {
                                    self.new_form_error = Some(format!("Failed to save: {e}"));
                                }
                            }
                        }
                        Err(e) => {
                            self.new_form_error = Some(format!("{e}"));
                        }
                    }
                }

                if ui.button("Cancel").clicked() {
                    self.show_new_dialog = false;
                    self.new_form_error = None;
                }
            });
        });
        ui.add_space(8.0);
    }
}
