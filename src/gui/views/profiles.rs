use crate::backend::config::{InterfaceConfig, PeerConfig, VpnProfile, WireGuardConfig};
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::storage::{AppSettings, ProfileStore};
use crate::backend::tunnel::TunnelState;
use crate::gui::theme::{
    card_frame, COLOR_ACCENT, COLOR_CONNECTED, COLOR_ERROR, COLOR_SURFACE_LIGHT, COLOR_TEXT,
    COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Button, Color32, RichText, ScrollArea, TextEdit, Vec2};

pub struct ProfilesView {
    search_query: String,
    show_new_dialog: bool,
    show_credentials_dialog: bool,

    // New profile form state
    new_name: String,
    new_private_key: String,
    new_address: String,
    new_dns: String,
    new_public_key: String,
    new_endpoint: String,
    new_form_error: Option<String>,

    // Quick credentials form state
    cred_private_key: String,
    cred_address: String,
    cred_msg: Option<String>,
}

impl Default for ProfilesView {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            show_new_dialog: false,
            show_credentials_dialog: false,
            new_name: String::new(),
            new_private_key: String::new(),
            new_address: "10.13.1.2/32".to_string(),
            new_dns: "1.1.1.1, 1.0.0.1".to_string(),
            new_public_key: String::new(),
            new_endpoint: String::new(),
            new_form_error: None,
            cred_private_key: String::new(),
            cred_address: "10.13.1.2/32".to_string(),
            cred_msg: None,
        }
    }
}

impl ProfilesView {
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        service: &VpnBackendService,
        store: &ProfileStore,
        settings: &mut AppSettings,
        profiles: &mut Vec<VpnProfile>,
        selected_profile_id: &mut String,
    ) {
        let current_state = service.current_state.lock().unwrap().clone();
        let _is_connected = matches!(current_state, TunnelState::Connected { .. });

        // Action Toolbar
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔍").size(15.0));
                ui.add(
                    TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search country, city, or server...")
                        .desired_width(220.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("➕ New Profile").strong()).clicked() {
                        self.show_new_dialog = true;
                        self.new_form_error = None;
                    }

                    if ui.button(RichText::new("📁 Import .conf").strong()).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("WireGuard Configuration", &["conf"])
                            .pick_file()
                        {
                            match store.import_file(&path) {
                                Ok(imported) => {
                                    *selected_profile_id = imported.id.clone();
                                    *profiles = store.load_all_profiles(settings);
                                }
                                Err(err) => {
                                    eprintln!("Import error: {err}");
                                }
                            }
                        }
                    }

                    if ui.button(RichText::new("🔑 Account Credentials").color(COLOR_ACCENT)).clicked() {
                        self.cred_private_key = settings.user_private_key.clone();
                        self.cred_address = settings.user_assigned_ip.clone();
                        self.show_credentials_dialog = true;
                        self.cred_msg = None;
                    }
                });
            });
        });

        ui.add_space(10.0);

        // Account Credentials Dialog
        if self.show_credentials_dialog {
            self.render_credentials_dialog(ui, store, settings, profiles);
        }

        // New Profile Modal Dialog
        if self.show_new_dialog {
            self.render_new_profile_dialog(ui, store, settings, profiles, selected_profile_id);
        }

        // Filter profiles
        let query = self.search_query.trim().to_lowercase();
        let filtered_profiles: Vec<(usize, VpnProfile)> = profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if query.is_empty() {
                    return true;
                }
                p.name.to_lowercase().contains(&query)
                    || p.country.to_lowercase().contains(&query)
                    || p.city.to_lowercase().contains(&query)
                    || p.country_code.to_lowercase().contains(&query)
                    || p.display_endpoint().to_lowercase().contains(&query)
            })
            .map(|(i, p)| (i, p.clone()))
            .collect();

        // Profiles List Scroll Area
        ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if filtered_profiles.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No VPN profiles found matching your search.").color(COLOR_TEXT_MUTED));
                });
                return;
            }

            for (_, profile) in filtered_profiles {
                let is_selected = &profile.id == selected_profile_id;
                let is_active_tunnel = match &current_state {
                    TunnelState::Connected { profile_name, .. } => profile_name == &profile.name,
                    _ => false,
                };

                let stroke_color = if is_active_tunnel {
                    COLOR_CONNECTED
                } else if is_selected {
                    COLOR_ACCENT
                } else {
                    Color32::from_rgb(51, 65, 85)
                };

                egui::Frame::new()
                    .fill(if is_selected { COLOR_SURFACE_LIGHT } else { Color32::from_rgb(24, 33, 47) })
                    .corner_radius(egui::CornerRadius::same(8))
                    .stroke(egui::Stroke::new(1.2, stroke_color))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Country Badge
                            let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(34.0, 24.0), egui::Sense::hover());
                            ui.painter().rect_filled(badge_rect, 4.0, Color32::from_rgb(15, 23, 42));
                            ui.painter().text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &profile.country_code,
                                egui::FontId::proportional(12.0),
                                COLOR_ACCENT,
                            );

                            ui.add_space(6.0);

                            // Server Details
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&profile.name).strong().size(14.0).color(COLOR_TEXT));
                                    if profile.is_preset {
                                        ui.label(RichText::new("Preset").size(11.0).color(COLOR_TEXT_MUTED));
                                    } else {
                                        ui.label(RichText::new("Custom").size(11.0).color(COLOR_ACCENT));
                                    }
                                });
                                ui.label(
                                    RichText::new(format!("{} • {}", profile.city, profile.display_endpoint()))
                                        .size(11.5)
                                        .color(COLOR_TEXT_MUTED),
                                );
                            });

                            // Action Buttons
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
                                    if ui.add(btn).clicked() {
                                        *selected_profile_id = profile.id.clone();
                                        service.send_command(ServiceCommand::Connect(profile.clone()));
                                    }
                                }

                                if !is_selected && !is_active_tunnel {
                                    if ui.button(RichText::new("Select").size(12.0)).clicked() {
                                        *selected_profile_id = profile.id.clone();
                                    }
                                }

                                if !profile.is_preset && !is_active_tunnel {
                                    if ui.button(RichText::new("🗑").size(12.0).color(COLOR_ERROR)).clicked() {
                                        let _ = store.delete_profile(&profile.id);
                                        *profiles = store.load_all_profiles(settings);
                                        if selected_profile_id == &profile.id {
                                            if let Some(first) = profiles.first() {
                                                *selected_profile_id = first.id.clone();
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(6.0);
            }
        });
    }

    fn render_credentials_dialog(
        &mut self,
        ui: &mut egui::Ui,
        store: &ProfileStore,
        settings: &mut AppSettings,
        profiles: &mut Vec<VpnProfile>,
    ) {
        card_frame().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔑 FastestVPN Account Configuration").strong().size(15.0).color(COLOR_TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").clicked() {
                            self.show_credentials_dialog = false;
                        }
                    });
                });

                ui.label(
                    RichText::new("FastestVPN assigns a unique WireGuard Private Key and internal IP to your account. Entering them here automatically activates all built-in server presets!")
                        .size(12.0)
                        .color(COLOR_TEXT_MUTED),
                );

                ui.add_space(8.0);

                ui.label(RichText::new("WireGuard PrivateKey (44 chars base64):").size(12.0).color(COLOR_TEXT));
                ui.add(TextEdit::singleline(&mut self.cred_private_key).hint_text("e.g. YWFhYWFhY...=").desired_width(450.0));

                ui.add_space(4.0);
                ui.label(RichText::new("Assigned Client Address / IP:").size(12.0).color(COLOR_TEXT));
                ui.add(TextEdit::singleline(&mut self.cred_address).hint_text("e.g. 10.13.1.2/32").desired_width(250.0));

                if let Some(ref msg) = self.cred_msg {
                    ui.label(RichText::new(msg).color(COLOR_CONNECTED).size(12.0));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Apply & Update Presets").strong().color(Color32::WHITE)).clicked() {
                        settings.user_private_key = self.cred_private_key.trim().to_string();
                        settings.user_assigned_ip = self.cred_address.trim().to_string();
                        let _ = store.save_settings(settings);
                        *profiles = store.load_all_profiles(settings);
                        self.cred_msg = Some("Credentials saved! All FastestVPN presets updated.".to_string());
                    }

                    if ui.button("Close").clicked() {
                        self.show_credentials_dialog = false;
                    }
                });
            });
        });
        ui.add_space(10.0);
    }

    fn render_new_profile_dialog(
        &mut self,
        ui: &mut egui::Ui,
        store: &ProfileStore,
        settings: &AppSettings,
        profiles: &mut Vec<VpnProfile>,
        selected_profile_id: &mut String,
    ) {
        card_frame().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("➕ Create WireGuard Profile").strong().size(15.0).color(COLOR_TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").clicked() {
                            self.show_new_dialog = false;
                        }
                    });
                });

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Profile Name:");
                    ui.add(TextEdit::singleline(&mut self.new_name).hint_text("e.g. Germany Dedicated").desired_width(220.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Endpoint (host:port):");
                    ui.add(TextEdit::singleline(&mut self.new_endpoint).hint_text("e.g. de-01.jumptoserver.com:51820").desired_width(260.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Server PublicKey:");
                    ui.add(TextEdit::singleline(&mut self.new_public_key).hint_text("44-char base64 server key").desired_width(320.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Client PrivateKey:");
                    ui.add(TextEdit::singleline(&mut self.new_private_key).hint_text("44-char base64 private key").desired_width(320.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Address:");
                    ui.add(TextEdit::singleline(&mut self.new_address).hint_text("10.13.1.2/32").desired_width(160.0));
                    ui.label("DNS:");
                    ui.add(TextEdit::singleline(&mut self.new_dns).hint_text("1.1.1.1, 1.0.0.1").desired_width(160.0));
                });

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
        });
        ui.add_space(10.0);
    }
}
