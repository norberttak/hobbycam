#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window on Windows release builds

mod aci;
mod canvas;
mod hit;
mod panels;
#[cfg(test)]
mod shot;
mod state;
mod tool_dialog;
mod view;

use eframe::egui;
use state::{AppState, NewKind};
use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut state = AppState::new(hc_project::default_library_path());
    if let Some(arg) = std::env::args_os().nth(1) {
        state.open_cli(&PathBuf::from(arg));
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(state.title())
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "HobbyCAM",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(state)))),
    )
}

/// Action waiting behind the "Save changes?" guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    New,
    Open,
    Quit,
}

pub struct App {
    pub state: AppState,
    pub tool_dialog: tool_dialog::ToolDialog,
    /// Inline "New operation" form: kind and tool id.
    pub new_op: Option<(NewKind, u32)>,
    pub pending: Option<Pending>,
    allow_close: bool,
    title: String,
    /// Screen position where a left-button box drag started.
    pub box_start: Option<egui::Pos2>,
    /// A left-button drag that started on a tab marker moves that tab.
    pub tab_drag: bool,
}

impl App {
    pub fn new(state: AppState) -> App {
        App {
            state,
            tool_dialog: Default::default(),
            new_op: None,
            pending: None,
            allow_close: false,
            title: String::new(),
            box_start: None,
            tab_drag: false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.state.refresh_toolpaths();
        let ctx = ui.ctx().clone();
        let title = self.state.title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.state.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        panels::shortcuts(self, &ctx);
        panels::menu(self, ui);
        panels::messages(self, ui);
        panels::status_bar(self, ui);
        panels::left(self, ui);
        panels::right(self, ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| canvas::show(self, ui));
        self.guard_modal(&ctx);
        tool_dialog::show(self, &ctx);
    }

    /// Runs `p` now, or asks first when there are unsaved changes.
    pub fn request(&mut self, ctx: &egui::Context, p: Pending) {
        if self.state.dirty {
            self.pending = Some(p);
        } else {
            self.perform(ctx, p);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, p: Pending) {
        match p {
            Pending::New => self.state.new_project(),
            Pending::Open => {
                if let Some(f) = rfd::FileDialog::new()
                    .add_filter("HobbyCAM project", &["hcam"])
                    .pick_file()
                {
                    self.state.open(&f);
                }
            }
            Pending::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Save to the current path, or ask for one.
    pub fn save(&mut self) -> bool {
        if self.state.path.is_some() {
            self.state.save()
        } else {
            self.save_as()
        }
    }

    pub fn save_as(&mut self) -> bool {
        let Some(mut f) = rfd::FileDialog::new()
            .add_filter("HobbyCAM project", &["hcam"])
            .set_file_name("untitled.hcam")
            .save_file()
        else {
            return false;
        };
        if !f
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("hcam"))
        {
            f.set_extension("hcam");
        }
        self.state.save_as(&f)
    }

    pub fn import_dxf(&mut self) {
        if let Some(f) = rfd::FileDialog::new()
            .add_filter("DXF drawing", &["dxf", "DXF"])
            .pick_file()
        {
            self.state.import_dxf(&f);
        }
    }

    fn guard_modal(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending else { return };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("unsaved_guard")).show(ctx, |ui| {
            ui.heading("Save changes?");
            ui.label("The project has unsaved changes.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Discard").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(2);
        }
        match choice {
            Some(0) => {
                self.pending = None;
                if self.save() {
                    self.perform(ctx, p);
                }
            }
            Some(1) => {
                self.pending = None;
                self.perform(ctx, p);
            }
            Some(_) => self.pending = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod kittest {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn harness() -> Harness<'static, App> {
        let mut s = AppState::new(None);
        s.open_cli(&state::tests::bracket_path());
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
    }

    /// Every non-ASCII character used in UI strings must be in egui's bundled fonts (the harness panics otherwise).
    #[test]
    fn ui_glyphs_in_default_fonts() {
        let mut h = Harness::new_ui(|ui| {
            ui.label("° × Ø – … ·");
        });
        h.run();
    }

    #[test]
    fn k1_layer_labels() {
        let h = harness();
        for name in ["Outline", "Center", "Hidden", "Title"] {
            assert!(h.query_by_label(name).is_some(), "{name}");
        }
    }

    #[test]
    fn k2_hide_layer_checkbox() {
        let mut h = harness();
        h.get_by_label("Show layer Hidden").click();
        h.run();
        assert!(h.state().state.hidden_layers.contains("Hidden"));
    }

    #[test]
    fn k3_file_menu() {
        let mut h = harness();
        h.get_by_label("File").click();
        h.run();
        assert!(h.query_by_label_contains("Import DXF…").is_some());
    }
}
