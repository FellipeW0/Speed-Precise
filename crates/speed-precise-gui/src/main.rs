#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! `speed-precise-gui`: Native Rust desktop studio (`eframe` + `egui` + `egui_plot`)
//! unifying DIView live calibration, WheelCheck `Step Log 2` (`1000 Hz`), and LUT Generator.

use std::sync::Arc;

pub mod app;
pub mod app_icon;
pub mod worker;

use app::SpeedPreciseApp;

fn main() -> eframe::Result<()> {
    let window_title = format!(
        "Speed Precise v{} — DirectInput8 Calibration, FFB Step Log 2 & LUT Studio",
        env!("CARGO_PKG_VERSION")
    );
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(window_title)
        .with_inner_size([1240.0, 820.0])
        .with_min_inner_size([960.0, 640.0]);
    if let Some(icon) = app_icon::load_app_icon_data() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Speed Precise",
        native_options,
        Box::new(|cc| Ok(Box::new(SpeedPreciseApp::new(cc)))),
    )
}
