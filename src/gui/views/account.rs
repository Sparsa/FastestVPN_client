use crate::backend::config::VpnProfile;
use crate::backend::service::{ServiceCommand, VpnBackendService};
use crate::backend::storage::{AppSettings, ProfileStore};
use crate::gui::theme::{card_frame, COLOR_ACCENT, COLOR_CONNECTED, COLOR_ERROR, COLOR_TEXT_MUTED};
use eframe::egui::{self, Color32, RichText, TextEdit};

/// Account login and WireGuard credentials, shown in the side pane.
pub struct AccountView {
    login_username: String,
    login_password: String,
    show_password: bool,
    login_msg: Option<Result<String, String>>,

    cred_private_key: String,
    cred_address: String,
    cred_msg: Option<String>,

    /// Form fields are filled from settings the first time the view is shown.
    loaded: bool,
}

impl Default for AccountView {
    fn default() -> Self {
        Self {
            login_username: String::new(),
            login_password: String::new(),
            show_password: false,
            login_msg: None,
            cred_private_key: String::new(),
            cred_address: "10.13.1.2/32".to_string(),
            cred_msg: None,
            loaded: false,
        }
    }
}

impl AccountView {
    /// Shows the outcome of an API login.
    pub fn set_login_feedback(&mut self, result: Result<String, String>) {
        self.login_msg = Some(result);
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        service: &VpnBackendService,
        store: &ProfileStore,
        settings: &mut AppSettings,
        profiles: &mut Vec<VpnProfile>,
    ) {
        if !self.loaded {
            self.login_username = settings.username.clone();
            self.login_password = settings.password.clone();
            self.cred_private_key = settings.user_private_key.clone();
            self.cred_address = settings.user_assigned_ip.clone();
            self.loaded = true;
        }

        let field_width = ui.available_width() - 28.0;

        // Section 1: Username & Password Authentication (for OpenVPN and API verification)
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("FastestVPN Login").strong().size(14.0).color(COLOR_ACCENT));
            ui.label(
                RichText::new("Used for OpenVPN tunnels and to verify your subscription.")
                    .size(11.5)
                    .color(COLOR_TEXT_MUTED),
            );

            ui.add_space(8.0);
            ui.label("Account Email");
            ui.add(
                TextEdit::singleline(&mut self.login_username)
                    .hint_text("user@example.com")
                    .desired_width(field_width),
            );

            ui.add_space(4.0);
            ui.label("Password");
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.login_password)
                        .password(!self.show_password)
                        .hint_text("password")
                        .desired_width(field_width - 70.0),
                );
                ui.checkbox(&mut self.show_password, "Show");
            });

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("🔐 Sign In & Verify").strong()).clicked() {
                    settings.username = self.login_username.trim().to_string();
                    settings.password = self.login_password.trim().to_string();
                    let _ = store.save_settings(settings);
                    self.login_msg = Some(Ok("Verifying account…".to_string()));

                    service.send_command(ServiceCommand::LoginApi {
                        username: settings.username.clone(),
                        password: settings.password.clone(),
                    });
                }

                if !settings.username.is_empty()
                    && ui.button(RichText::new("Log Out").color(COLOR_ERROR)).clicked()
                {
                    settings.username.clear();
                    settings.password.clear();
                    settings.auth_token = None;
                    settings.account_status = None;
                    settings.expiry_date = None;
                    settings.plan_name = None;
                    self.login_msg = None;
                    self.login_username.clear();
                    self.login_password.clear();
                    let _ = store.save_settings(settings);
                }
            });

            match &self.login_msg {
                Some(Err(err)) => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("✖ {err}")).color(COLOR_ERROR).size(12.0));
                }
                Some(Ok(msg)) => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg).color(COLOR_TEXT_MUTED).size(12.0));
                }
                None => {}
            }

            if let Some(ref status) = settings.account_status {
                ui.add_space(4.0);
                let expiry = settings.expiry_date.as_deref().unwrap_or("—");
                ui.label(
                    RichText::new(format!("✔ Status: {status} • Expiry: {expiry}"))
                        .color(COLOR_CONNECTED)
                        .size(12.0),
                );
                if let Some(ref plan) = settings.plan_name {
                    ui.label(RichText::new(format!("Plan: {plan}")).color(COLOR_TEXT_MUTED).size(12.0));
                }
            }
        });

        ui.add_space(10.0);

        // Section 2: WireGuard Cryptographic Keypair (for WireGuard Protocol)
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("WireGuard Keys").strong().size(14.0).color(COLOR_ACCENT));
            ui.label(
                RichText::new("Optional. Activates the built-in servers for the WireGuard protocol.")
                    .size(11.5)
                    .color(COLOR_TEXT_MUTED),
            );

            ui.add_space(8.0);
            ui.label("PrivateKey (44-char Base64)");
            ui.add(
                TextEdit::singleline(&mut self.cred_private_key)
                    .password(true)
                    .hint_text("YWFhYWFhY...=")
                    .desired_width(field_width),
            );

            ui.add_space(4.0);
            ui.label("Assigned Client Address");
            ui.add(
                TextEdit::singleline(&mut self.cred_address)
                    .hint_text("10.13.1.2/32")
                    .desired_width(field_width),
            );

            ui.add_space(8.0);
            if ui.button(RichText::new("Save Keys").strong().color(Color32::WHITE)).clicked() {
                settings.user_private_key = self.cred_private_key.trim().to_string();
                settings.user_assigned_ip = self.cred_address.trim().to_string();
                let _ = store.save_settings(settings);
                *profiles = store.load_all_profiles(settings);
                self.cred_msg = Some("WireGuard keys saved.".to_string());
            }

            if let Some(ref msg) = self.cred_msg {
                ui.add_space(4.0);
                ui.label(RichText::new(msg).color(COLOR_CONNECTED).size(12.0));
            }
        });

        ui.add_space(6.0);
        ui.label(
            RichText::new("Credentials are stored in ~/.config/fastestvpn-gui with 0600 permissions.")
                .size(11.0)
                .color(COLOR_TEXT_MUTED),
        );
    }
}
