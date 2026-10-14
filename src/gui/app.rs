use crate::backend::config::{VpnProfile, VpnProtocol};
use crate::backend::service::{ServiceEvent, VpnBackendService};
use crate::backend::storage::{AppSettings, ProfileStore};
use crate::backend::tunnel::TunnelState;
use crate::gui::theme::{
    setup_custom_theme, COLOR_ACCENT, COLOR_BG, COLOR_CONNECTED, COLOR_CONNECTING, COLOR_ERROR,
    COLOR_SURFACE, COLOR_TEXT_MUTED,
};
use crate::gui::views::{
    AccountView, DashboardAction, DashboardView, DiagnosticsView, LogsView, ProfilesView,
};
use eframe::egui::{self, Align, Layout, RichText, Vec2};
use std::time::Duration;

/// Sections of the right-hand sliding pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSection {
    Servers,
    Account,
    System,
    Logs,
}

pub struct FastestVpnApp {
    service: VpnBackendService,
    store: ProfileStore,
    settings: AppSettings,
    profiles: Vec<VpnProfile>,
    selected_profile_id: String,
    /// Protocol the loaded presets were built with; tracks picker changes.
    applied_protocol: VpnProtocol,

    // Side pane
    pane_open: bool,
    pane_section: PaneSection,

    // Views
    profiles_view: ProfilesView,
    account_view: AccountView,
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
            .filter(|id| profiles.iter().any(|p| &p.id == id))
            .or_else(|| profiles.first().map(|p| p.id.clone()))
            .unwrap_or_else(|| "fastestvpn".to_string());

        Self {
            service,
            store,
            settings,
            profiles,
            selected_profile_id,
            applied_protocol,
            pane_open: false,
            pane_section: PaneSection::Servers,
            profiles_view: ProfilesView::default(),
            account_view: AccountView::default(),
            diagnostics_view: DiagnosticsView::default(),
            logs_view: LogsView::default(),
        }
    }

    fn open_pane(&mut self, section: PaneSection) {
        self.pane_section = section;
        self.pane_open = true;
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top_navigation")
            .frame(
                egui::Frame::new()
                    .fill(COLOR_SURFACE)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚡ FastestVPN").size(17.0).strong().color(COLOR_ACCENT));
                    ui.label(RichText::new("Linux Desktop").size(11.0).color(COLOR_TEXT_MUTED));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let menu_label = if self.pane_open { "✖" } else { "☰" };
                        if ui
                            .add(egui::Button::new(RichText::new(menu_label).size(16.0)).min_size(Vec2::new(32.0, 28.0)))
                            .on_hover_text("Servers, account & settings")
                            .clicked()
                        {
                            self.pane_open = !self.pane_open;
                        }

                        ui.add_space(8.0);

                        let account_label = if self.settings.username.is_empty() {
                            RichText::new("👤 Sign in").size(12.0).color(COLOR_TEXT_MUTED)
                        } else {
                            RichText::new(format!("👤 {}", self.settings.username)).size(12.0).color(COLOR_ACCENT)
                        };
                        if ui.add(egui::Button::new(account_label).frame(false)).clicked() {
                            self.open_pane(PaneSection::Account);
                        }

                        ui.add_space(8.0);

                        let current_state = self.service.current_state.lock().unwrap().clone();
                        let (dot_color, label) = match current_state {
                            TunnelState::Connected { .. } => (COLOR_CONNECTED, "Protected"),
                            TunnelState::Connecting { .. } => (COLOR_CONNECTING, "Connecting"),
                            TunnelState::Disconnecting => (COLOR_CONNECTING, "Disconnecting"),
                            TunnelState::Disconnected => (COLOR_TEXT_MUTED, "Disconnected"),
                            TunnelState::Error { .. } => (COLOR_ERROR, "Error"),
                        };
                        ui.label(RichText::new(label).size(12.0).color(COLOR_TEXT_MUTED));
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.5, dot_color);
                    });
                });
            });
    }

    fn render_side_pane(&mut self, ui: &mut egui::Ui) {
        let mut pane_open = self.pane_open;
        egui::Panel::right("options_pane")
            .resizable(true)
            .default_size(400.0)
            .size_range(340.0..=560.0)
            .frame(
                egui::Frame::new()
                    .fill(COLOR_SURFACE)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show_collapsible(ui, &mut pane_open, |ui| {
                // Section switcher
                ui.horizontal(|ui| {
                    for (section, title) in [
                        (PaneSection::Servers, "🌍 Servers"),
                        (PaneSection::Account, "👤 Account"),
                        (PaneSection::System, "🛠 System"),
                        (PaneSection::Logs, "📜 Logs"),
                    ] {
                        let active = self.pane_section == section;
                        let text = RichText::new(title)
                            .size(13.0)
                            .strong()
                            .color(if active { COLOR_ACCENT } else { COLOR_TEXT_MUTED });
                        if ui.selectable_label(active, text).clicked() {
                            self.pane_section = section;
                        }
                    }
                });
                ui.separator();
                ui.add_space(4.0);

                match self.pane_section {
                    PaneSection::Servers => {
                        let connect_requested = self.profiles_view.render(
                            ui,
                            &self.service,
                            &self.store,
                            &mut self.settings,
                            &mut self.profiles,
                            &mut self.selected_profile_id,
                        );
                        // Slide the pane away so the dashboard shows the connection progress.
                        if connect_requested {
                            self.pane_open = false;
                        }
                    }
                    PaneSection::Account => {
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            self.account_view.render(
                                ui,
                                &self.service,
                                &self.store,
                                &mut self.settings,
                                &mut self.profiles,
                            );
                        });
                    }
                    PaneSection::System => {
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            self.diagnostics_view.render(ui, &self.service);
                        });
                    }
                    PaneSection::Logs => self.logs_view.render(ui, &self.service),
                }
            });
        // The panel may close itself (drag-to-collapse); a connect click also closes it.
        if !pane_open {
            self.pane_open = false;
        }
    }
}

impl eframe::App for FastestVpnApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll events from the backend service worker
        let mut settings_dirty = false;
        let mut profiles_dirty = false;
        let account_view = &mut self.account_view;
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
                account_view.set_login_feedback(Ok("Login verified.".to_string()));
                settings_dirty = true;
            }
            ServiceEvent::LoginResult(Err(err)) => {
                account_view.set_login_feedback(Err(err));
            }
            _ => {}
        });

        // Presets take their protocol from the pane's picker; persist and re-apply it.
        if self.settings.preferred_protocol != self.applied_protocol {
            self.applied_protocol = self.settings.preferred_protocol;
            settings_dirty = true;
            profiles_dirty = true;
        }

        // Remember the chosen server across restarts.
        if self.settings.default_profile_id.as_deref() != Some(self.selected_profile_id.as_str()) {
            self.settings.default_profile_id = Some(self.selected_profile_id.clone());
            settings_dirty = true;
        }

        if settings_dirty {
            let _ = self.store.save_settings(&self.settings);
        }
        if profiles_dirty {
            self.profiles = self.store.load_all_profiles(&self.settings);
        }

        // Request repaint periodically for smooth real-time metric counter updates
        ui.ctx().request_repaint_after(Duration::from_millis(500));

        if self.pane_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.pane_open = false;
        }

        // Panels must be added before the central panel.
        self.render_top_bar(ui);
        self.render_side_pane(ui);

        let mut dashboard_action = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(COLOR_BG)
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    dashboard_action = DashboardView::render(
                        ui,
                        &self.service,
                        &self.settings,
                        &self.profiles,
                        &self.selected_profile_id,
                    );
                });
            });

        if dashboard_action == Some(DashboardAction::OpenServers) {
            self.open_pane(PaneSection::Servers);
        }
    }
}
