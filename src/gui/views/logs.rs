use crate::backend::service::{LogLevel, VpnBackendService};
use crate::gui::theme::{
    card_frame, COLOR_ACCENT, COLOR_CONNECTED, COLOR_CONNECTING, COLOR_ERROR, COLOR_TEXT,
    COLOR_TEXT_MUTED,
};
use eframe::egui::{self, Color32, RichText, ScrollArea};

pub struct LogsView {
    filter_errors_only: bool,
}

impl Default for LogsView {
    fn default() -> Self {
        Self {
            filter_errors_only: false,
        }
    }
}

impl LogsView {
    pub fn render(&mut self, ui: &mut egui::Ui, service: &VpnBackendService) {
        let logs = service.logs.lock().unwrap().clone();

        // Toolbar
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear Logs").clicked() {
                        service.logs.lock().unwrap().clear();
                    }

                    if ui.button("Copy All").clicked() {
                        let text = logs
                            .iter()
                            .map(|l| format!("[{}] {:?}: {}", l.timestamp, l.level, l.message))
                            .collect::<Vec<_>>()
                            .join("\n");
                        ui.ctx().copy_text(text);
                    }

                    ui.checkbox(&mut self.filter_errors_only, "Errors Only");
                });
            });
        });

        ui.add_space(10.0);

        // Terminal Log Frame
        egui::Frame::new()
            .fill(Color32::from_rgb(10, 15, 25))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(30, 41, 59)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        if logs.is_empty() {
                            ui.label(RichText::new("No log events recorded yet.").color(COLOR_TEXT_MUTED));
                            return;
                        }

                        for entry in &logs {
                            if self.filter_errors_only && entry.level != LogLevel::Error {
                                continue;
                            }

                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("[{}]", entry.timestamp))
                                        .color(COLOR_TEXT_MUTED)
                                        .monospace()
                                        .size(11.5),
                                );

                                let (lvl_str, lvl_color) = match entry.level {
                                    LogLevel::Info => ("INFO", COLOR_ACCENT),
                                    LogLevel::Success => ("OK", COLOR_CONNECTED),
                                    LogLevel::Warning => ("WARN", COLOR_CONNECTING),
                                    LogLevel::Error => ("ERROR", COLOR_ERROR),
                                };

                                ui.label(
                                    RichText::new(format!("[{lvl_str}]"))
                                        .color(lvl_color)
                                        .monospace()
                                        .strong()
                                        .size(11.5),
                                );

                                ui.label(
                                    RichText::new(&entry.message)
                                        .color(COLOR_TEXT)
                                        .monospace()
                                        .size(11.5),
                                );
                            });
                        }
                    });
            });
    }
}
