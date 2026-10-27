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
    /// Export modal: Error = "Cannot export G-code", Warning = "Export anyway?".
    pub export_prompt: Option<hc_toolpath::Severity>,
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
            export_prompt: None,
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
        panels::issues(self, ui);
        panels::left(self, ui);
        panels::right(self, ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| canvas::show(self, ui));
        self.guard_modal(&ctx);
        self.export_modal(&ctx);
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

    /// Fresh check first: errors refuse, warnings ask, clean goes straight to the file dialog.
    pub fn export_gcode(&mut self) {
        self.state.refresh_toolpaths();
        self.export_prompt = self.state.issues.iter().map(|i| i.severity).max();
        if self.export_prompt.is_none() {
            self.write_export();
        }
    }

    /// Runs after the check passed or the user confirmed the warnings; the chosen file name is the base of all segment files.
    fn write_export(&mut self) {
        let Some(files) = self.state.gcode() else {
            return;
        };
        // gcode() succeeded, so the post name is valid
        let Ok(post) = hc_post::post_for(&self.state.project.machine.post) else {
            return;
        };
        let ext = post.extension();
        let mut dialog = rfd::FileDialog::new()
            .add_filter("G-code", &[ext])
            .set_file_name(format!("{}.{ext}", self.state.stem()));
        if let Some(dir) = self.state.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(base) = dialog.save_file() {
            self.state.write_gcode(&base, &files);
        }
    }

    fn export_modal(&mut self, ctx: &egui::Context) {
        use hc_toolpath::Severity as S;
        let Some(sev) = self.export_prompt else {
            return;
        };
        let mut done = false;
        let mut export = false;
        let modal = egui::Modal::new(egui::Id::unique("export_prompt")).show(ctx, |ui| {
            ui.heading(match sev {
                S::Error => "Cannot export G-code",
                S::Warning => "Export anyway?",
            });
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for i in self.state.issues.iter().filter(|i| i.severity == sev) {
                        let (word, rest) = self.state.issue_parts(i);
                        ui.label(panels::issue_job(ui, word, &rest));
                    }
                });
            ui.horizontal(|ui| match sev {
                S::Error => done = ui.button("OK").clicked(),
                S::Warning => {
                    export = ui.button("Export anyway").clicked();
                    done = ui.button("Cancel").clicked();
                }
            });
        });
        if done || export || modal.should_close() {
            self.export_prompt = None;
        }
        if export {
            self.write_export();
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

    #[test]
    fn m7_k_export_and_machine_menu() {
        let mut h = harness();
        h.get_by_label("File").click();
        h.run();
        assert!(h.query_by_label_contains("Export G-code…").is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        h.get_by_label("Settings").click();
        h.run();
        h.get_by_label("Machine").hover();
        h.run();
        for name in ["Safe Z", "Clearance Z", "Tool change Z"] {
            assert!(h.query_by_label(name).is_some(), "{name}");
        }
    }

    fn harness_for(s: AppState) -> Harness<'static, App> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
    }

    /// M8 test 32.
    #[test]
    fn m8_t32_issues_panel_and_export_modals() {
        let (s, inside) = state::tests::bracket_issues();
        let mut h = harness_for(s);
        h.run();
        assert!(h.query_by_label_contains("Issues (2)").is_some());
        assert!(h
            .query_by_label(
                "Error · Contour 2 · Tool too large for the circle Ø15.875 mm on layer Outline"
            )
            .is_some());
        assert!(h
            .query_by_label("Warning · Contour 1 · Spindle speed is 0 (S0)")
            .is_some());
        assert!(h.query_by_label("1 error").is_some());
        assert!(h.query_by_label("1 warning").is_some());
        h.get_by_label_contains("Tool too large").click();
        h.run();
        assert_eq!(h.state().state.selected_op, Some(inside));
        assert_eq!(h.state().state.selection.len(), 1);

        // errors: refusal modal, no file dialog, nothing written
        h.state_mut().export_gcode();
        h.run();
        assert_eq!(h.state().export_prompt, Some(hc_toolpath::Severity::Error));
        assert!(h.query_by_label("Cannot export G-code").is_some());
        assert!(!h
            .state()
            .state
            .messages
            .iter()
            .any(|m| m.text.starts_with("Exported")));
        h.get_by_label("OK").click();
        h.run();
        assert_eq!(h.state().export_prompt, None);
        assert!(h.query_by_label("Cannot export G-code").is_none());

        // only warnings: "Export anyway?" with Cancel
        h.state_mut().state.delete_op(inside);
        h.run();
        h.state_mut().export_gcode();
        h.run();
        assert!(h.query_by_label("Export anyway?").is_some());
        assert!(h.query_by_label("Export anyway").is_some());
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().export_prompt, None);
        assert!(h.query_by_label("Export anyway?").is_none());
        // Escape closes it too
        h.state_mut().export_gcode();
        h.run();
        assert!(h.query_by_label("Export anyway?").is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        assert_eq!(h.state().export_prompt, None);
    }
}
