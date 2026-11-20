//! Tokyo Night color palette and egui theming.

use eframe::egui::{self, Color32, Visuals};

// Tokyo Night (storm/dark) palette.
pub const BACKGROUND: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x26);
pub const BACKGROUND_DIM: Color32 = Color32::from_rgb(0x16, 0x16, 0x1e);
pub const PANEL: Color32 = Color32::from_rgb(0x1f, 0x23, 0x35);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(0x41, 0x49, 0x8a);
pub const FOREGROUND: Color32 = Color32::from_rgb(0xc0, 0xca, 0xf5);
pub const FOREGROUND_DIM: Color32 = Color32::from_rgb(0x92, 0x9c, 0xb8);
pub const ACCENT: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x61, 0x82, 0xc4);
pub const GREEN: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);
pub const RED: Color32 = Color32::from_rgb(0xf7, 0x76, 0x7e);

/// Apply the Tokyo Night theme to an egui context.
pub fn apply(context: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = BACKGROUND_DIM;
    visuals.override_text_color = Some(FOREGROUND);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
    visuals.selection.stroke.color = ACCENT;
    visuals.widgets.inactive.bg_fill = PANEL;
    visuals.widgets.inactive.weak_bg_fill = PANEL;
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.hovered.weak_bg_fill = ACCENT.gamma_multiply(0.2);
    visuals.widgets.noninteractive.bg_stroke.color = PANEL_BORDER;
    visuals.widgets.noninteractive.fg_stroke.color = FOREGROUND_DIM;
    context.set_visuals(visuals);
}
