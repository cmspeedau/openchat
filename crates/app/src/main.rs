use eframe::egui;

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x1b, 0x26);
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x16, 0x16, 0x1e);
const FOREGROUND: egui::Color32 = egui::Color32::from_rgb(0xc0, 0xca, 0xf5);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x7a, 0xa2, 0xf7);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "openchat",
        options,
        Box::new(|_creation_context| Ok(Box::new(OpenChatApp))),
    )
}

struct OpenChatApp;

impl eframe::App for OpenChatApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BACKGROUND;
        visuals.faint_bg_color = BACKGROUND;
        visuals.override_text_color = Some(FOREGROUND);
        visuals.selection.bg_fill = ACCENT;
        context.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BACKGROUND))
            .show(context, |_ui| {});
    }
}
