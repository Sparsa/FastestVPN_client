use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke, Visuals};

pub const COLOR_BG: Color32 = Color32::from_rgb(15, 23, 42); // Slate 900
pub const COLOR_SURFACE: Color32 = Color32::from_rgb(30, 41, 59); // Slate 800
pub const COLOR_SURFACE_LIGHT: Color32 = Color32::from_rgb(51, 65, 85); // Slate 700
pub const COLOR_ACCENT: Color32 = Color32::from_rgb(6, 182, 212); // Cyan 500
pub const COLOR_ACCENT_HOVER: Color32 = Color32::from_rgb(34, 211, 238); // Cyan 400
pub const COLOR_CONNECTED: Color32 = Color32::from_rgb(34, 197, 94); // Green 500
pub const COLOR_CONNECTING: Color32 = Color32::from_rgb(234, 179, 8); // Yellow 500
pub const COLOR_ERROR: Color32 = Color32::from_rgb(239, 68, 68); // Red 500
pub const COLOR_TEXT: Color32 = Color32::from_rgb(248, 250, 252); // Slate 50
pub const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(148, 163, 184); // Slate 400

pub fn setup_custom_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = COLOR_BG;
    visuals.window_fill = COLOR_SURFACE;
    visuals.extreme_bg_color = Color32::from_rgb(10, 15, 30);

    // Rounded corners
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    // Backgrounds
    visuals.widgets.noninteractive.bg_fill = COLOR_SURFACE;
    visuals.widgets.inactive.bg_fill = COLOR_SURFACE_LIGHT;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(71, 85, 105);
    visuals.widgets.active.bg_fill = COLOR_ACCENT;

    // Stroke
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(51, 65, 85));
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(71, 85, 105));
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, COLOR_ACCENT);

    ctx.set_visuals(visuals);
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(COLOR_SURFACE)
        .corner_radius(CornerRadius::same(10))
        .stroke(Stroke::new(1.0, Color32::from_rgb(51, 65, 85)))
        .inner_margin(Margin::same(14))
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_rate(rate_bps: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    if rate_bps >= MB {
        format!("{:.2} MB/s", rate_bps / MB)
    } else if rate_bps >= KB {
        format!("{:.1} KB/s", rate_bps / KB)
    } else {
        format!("{:.0} B/s", rate_bps)
    }
}

pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}
