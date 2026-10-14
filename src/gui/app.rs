use crate::backend::config::{VpnProfile, VpnProtocol};
use crate::backend::service::{ServiceEvent, VpnBackendService};
use crate::backend::storage::{AppSettings, ProfileStore};
use crate::backend::tunnel::TunnelState;
use crate::gui::theme::{
    setup_custom_theme, COLOR_ACCENT, COLOR_CONNECTED, COLOR_CONNECTING, COLOR_ERROR, COLOR_SURFACE,
    COLOR_TEXT_MUTED,
};
use crate::gui::views::{DashboardView, DiagnosticsView, LogsView, ProfilesView};
use eframe::egui::{self, Align, Layout, RichText, Vec2};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Servers,
    Diagnostics,
    Logs,
}

pub struct FastestVpnApp {
    active_tab: Tab,
    service: VpnBackendService,
    store: ProfileStore,
    settings: AppSettings,
    profiles: Vec<VpnProfile>,
    selected_profile_id: String,
    /// Protocol the loaded presets were built with; tracks dashboard picker changes.
    applied_protocol: VpnProtocol,

    // Views
    profiles_view: ProfilesView,
    diagnostics_view: DiagnosticsView,
    logs_view: LogsView,
}

impl FastestVpnApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_theme(&cc.egui_ctx);

        let service = VpnBackendService::new();
        let store = ProfileStore::new();
        let settings = store.load_settings();
        let profiles = store.load_all_profiles(&settings);
        let applied_protocol = settings.preferred_protocol;

        let selected_profile_id = settings
            .default_profile_id
            .clone()
            .or_else(|| profiles.first().map(|p| p.id.clone()))
            .unwrap_or_else(|| "fastestvpn".to_string());

        Self {
            active_tab: Tab::Dashboard,
            service,
            store,
            settings,
            profiles,
            selected_profile_id,
            applied_protocol,
            profiles_view: ProfilesView::default(),
            diagnostics_view: DiagnosticsView::default(),
            logs_view: LogsView::default(),
        }
    }
}

impl eframe::App for FastestVpnApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll events from the backend service worker
        let mut settings_dirty = false;
        let mut profiles_dirty = false;
        let profiles_view = &mut self.profiles_view;
        let settings = &mut self.settings;
        self.service.poll_events(|event| match event {
            ServiceEvent::LoginResult(Ok((user, token))) => {
                if token.is_some() {
                    settings.auth_token = token;
                }
                if let Some(status) = user.status {
                    settings.account_status = Some(status);
                }
                if let Some(expiry) = user.expiry_date {
                    settings.expiry_date = Some(expiry);
                }
                if let Some(plan) = user.plan_name {
                    settings.plan_name = Some(plan);
                }
                // Provisioned WireGuard credentials activate the built-in presets.
                if let Some(key) = user.wireguard_private_key.filter(|k| !k.trim().is_empty()) {
                    settings.user_private_key = key.trim().to_string();
                    profiles_dirty = true;
                }
                if let Some(ip) = user.wireguard_ip.filter(|ip| !ip.trim().is_empty()) {
                    settings.user_assigned_ip = ip.trim().to_string();
                    profiles_dirty = true;
                }
                if settings.account_status.is_none() {
                    settings.account_status = Some("Active".to_string());
                }
                profiles_view.set_login_feedback(Ok("Login verified.".to_string()));
                settings_dirty = true;
            }
            ServiceEvent::LoginResult(Err(err)) => {
                profiles_view.set_login_feedback(Err(err));
            }
            _ => {}
        });

        // Presets take their protocol from the dashboard picker; persist and re-apply it.
        if self.settings.preferred_protocol != self.applied_protocol {
            self.applied_protocol = self.settings.preferred_protocol;
            settings_dirty = true;
            profiles_dirty = true;
        }

        if settings_dirty {
            let _ = self.store.save_settings(&self.settings);
        }
        if profiles_dirty {
            self.profiles = self.store.load_all_profiles(&self.settings);
        }

        // Request repaint periodically for smooth real-time metric counter updates
        ui.ctx().request_repaint_after(Duration::from_millis(500));

        // Top Navigation Bar
        egui::Panel::top("top_navigation")
            .frame(
                egui::Frame::new()
                    .fill(COLOR_SURFACE)
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Logo & App Title
                    ui.label(RichText::new("⚡ FastestVPN").size(17.0).strong().color(COLOR_ACCENT));
                    ui.label(RichText::new("Linux Desktop").size(11.0).color(COLOR_TEXT_MUTED));

                    ui.add_space(20.0);

                    // Tab Navigation Buttons
                    let tabs = [
                        (Tab::Dashboard, "📊 Dashboard"),
                        (Tab::Servers, "🌍 Servers"),
                        (Tab::Diagnostics, "🛠 Diagnostics"),
                        (Tab::Logs, "📜 Logs"),
                    ];

                    for (tab, title) in tabs {
                        let is_active = self.active_tab == tab;
                        let text = RichText::new(title).size(13.5).strong();
                        let text = if is_active {
                            text.color(COLOR_ACCENT)
                        } else {
                            text.color(COLOR_TEXT_MUTED)
                        };

                        if ui.selectable_label(is_active, text).clicked() {
                            self.active_tab = tab;
                        }
                    }

                    // Right side status & user pill
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let current_state = self.service.current_state.lock().unwrap().clone();
                        let (dot_color, label) = match current_state {
                            TunnelState::Connected { .. } => (COLOR_CONNECTED, "Protected"),
                            TunnelState::Connecting { .. } => (COLOR_CONNECTING, "Connecting"),
                            TunnelState::Disconnecting => (COLOR_CONNECTING, "Disconnecting"),
                            TunnelState::Disconnected => (COLOR_TEXT_MUTED, "Disconnected"),
                            TunnelState::Error { .. } => (COLOR_ERROR, "Error"),
                        };

                        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.5, dot_color);
                        ui.label(RichText::new(label).size(12.0).color(COLOR_TEXT_MUTED));

                        if !self.settings.username.is_empty() {
                            ui.label(
                                RichText::new(format!("👤 {}", self.settings.username))
                                    .size(12.0)
                                    .color(COLOR_ACCENT),
                            );
                        }
                    });
                });
            });

        // Main Content Area
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(crate::gui::theme::COLOR_BG)
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ui, |ui| match self.active_tab {
                Tab::Dashboard => {
                    DashboardView::render(
                        ui,
                        &self.service,
                        &mut self.settings,
                        &self.profiles,
                        &mut self.selected_profile_id,
                    );
                }
                Tab::Servers => {
                    self.profiles_view.render(
                        ui,
                        &self.service,
                        &self.store,
                        &mut self.settings,
                        &mut self.profiles,
                        &mut self.selected_profile_id,
                    );
                }
                Tab::Diagnostics => {
                    self.diagnostics_view.render(ui, &self.service);
                }
                Tab::Logs => {
                    self.logs_view.render(ui, &self.service);
                }
            });
    }
}
