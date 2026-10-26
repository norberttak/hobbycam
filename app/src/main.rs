#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window on Windows release builds

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HobbyCAM")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "HobbyCAM",
        options,
        Box::new(|_cc| Ok(Box::<HobbyCamApp>::default())),
    )
}

#[derive(Default)]
struct HobbyCamApp {}

impl eframe::App for HobbyCamApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(format!("HobbyCAM {}", env!("CARGO_PKG_VERSION")));
        });
    }
}
