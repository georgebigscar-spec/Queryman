// Без консольного окна в release-сборке Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fonts;
mod state;
mod ui;
mod worker;

use eframe::egui;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(queryman_core::APP_NAME)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        queryman_core::APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::QuerymanApp::new(cc)))),
    )
}
