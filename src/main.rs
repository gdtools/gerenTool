mod app;
mod menu;
mod pages;
mod screenshot;

use app::SettingsApp;
use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Settings App",
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
    )
}
