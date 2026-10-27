#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window on Windows release builds

mod aci;
mod canvas;
#[cfg(test)]
mod docs_check;
mod hit;
#[cfg(test)]
mod icon_gen;
mod icons;
#[cfg(test)]
mod m10_verify;
#[cfg(test)]
mod m10a2_verify;
#[cfg(test)]
mod m10b_verify;
mod panels;
#[cfg(test)]
mod release_check;
#[cfg(test)]
mod shot;
mod snap;
mod state;
mod tool_dialog;
mod view;

use eframe::egui;
use state::{AppState, NewKind};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> eframe::Result {
    let mut state = AppState::new(hc_project::default_library_path());
    if let Some(arg) = std::env::args_os().nth(1) {
        state.open_cli(&PathBuf::from(arg));
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(state.title())
            .with_inner_size([1280.0, 800.0])
            .with_icon(Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../assets/icon/hobbycam-256.png"
                ))
                .expect("icon"),
            )),
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
    /// The icon font is installed (once, on the first frame).
    fonts_ready: bool,
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
            fonts_ready: false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.state.refresh_toolpaths();
        let ctx = ui.ctx().clone();
        if !self.fonts_ready {
            icons::install(&ctx);
            self.fonts_ready = true;
        }
        if self.state.sim_view.playing {
            self.state.advance(ui.input(|i| i.stable_dt) as f64);
            ctx.request_repaint();
        }
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
        panels::sim_panel(self, ui);
        panels::issues(self, ui);
        panels::sidebar(self, ui);
        panels::left(self, ui);
        panels::right(self, ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| canvas::show(self, ui));
        self.guard_modal(&ctx);
        self.export_modal(&ctx);
        tool_dialog::show(self, &ctx);
        // one undo step per finished gesture: a drag or typing commits on release / focus loss
        let idle = !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input();
        self.state.commit_undo(idle);
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
        // check before the dialog: "nothing to export" and errors fail without asking for a file
        let stem = self.state.stem();
        if self.state.gcode(&stem).is_none() {
            return;
        }
        // gcode() succeeded, so the post name is valid
        let Ok(post) = hc_post::post_for(&self.state.project.machine.post) else {
            return;
        };
        let ext = post.extension();
        // sandbox: a folder grant covers every segment file, its .bak and the stale-file scan
        #[cfg(feature = "appstore")]
        let base = rfd::FileDialog::new()
            .set_title("Choose the folder for the G-code files")
            .pick_folder()
            .map(|folder| folder.join(format!("{stem}.{ext}")));
        #[cfg(not(feature = "appstore"))]
        let base = {
            let mut dialog = rfd::FileDialog::new()
                .add_filter("G-code", &[ext])
                .set_file_name(format!("{}.{ext}", self.state.stem()));
            if let Some(dir) = self.state.path.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.set_directory(dir);
            }
            dialog.save_file()
        };
        if let Some(base) = base {
            // headers carry the chosen file name
            let name = base
                .file_stem()
                .map_or(stem, |n| n.to_string_lossy().into_owned());
            if let Some(files) = self.state.gcode(&name) {
                self.state.write_gcode(&base, &files);
            }
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

    /// Sidebar buttons are the 40 px squares of the two left columns (menu popups can open over the sidebar).
    fn in_sidebar(n: &egui_kittest::Node<'_>) -> bool {
        let r = n.rect();
        r.min.x < 110.0 && r.height() >= 36.0 && r.width() <= 44.0
    }

    /// The node named (or containing) `name` outside the sidebar; menus and the sidebar share names.
    pub(crate) fn menu_q<'a>(
        h: &'a Harness<'_, App>,
        name: &'a str,
        contains: bool,
    ) -> Option<egui_kittest::Node<'a>> {
        let mut v: Vec<_> = if contains {
            h.query_all_by_label_contains(name).collect()
        } else {
            h.query_all_by_label(name).collect()
        };
        v.retain(|n| !in_sidebar(n));
        assert!(v.len() <= 1, "{name}: {} nodes", v.len());
        v.pop()
    }

    pub(crate) fn menu<'a>(h: &'a Harness<'_, App>, name: &'a str) -> egui_kittest::Node<'a> {
        menu_q(h, name, false).unwrap_or_else(|| panic!("no node {name}"))
    }

    /// The sidebar button named `name`.
    pub(crate) fn side<'a>(h: &'a Harness<'_, App>, name: &'a str) -> egui_kittest::Node<'a> {
        let mut v: Vec<_> = h.query_all_by_label(name).filter(in_sidebar).collect();
        assert_eq!(v.len(), 1, "sidebar {name}");
        v.pop().unwrap()
    }

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
            assert!(menu_q(&h, name, false).is_some(), "{name}");
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
        assert!(menu_q(&h, "Import DXF…", true).is_some());
    }

    #[test]
    fn m7_k_export_and_machine_menu() {
        let mut h = harness();
        h.get_by_label("File").click();
        h.run();
        assert!(menu_q(&h, "Export G-code…", true).is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        h.get_by_label("Settings").click();
        h.run();
        menu(&h, "Machine").hover();
        h.run();
        for name in ["Safe Z", "Clearance Z", "Tool change Z"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
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

    /// M9 test 17.
    #[test]
    fn m9_t17_sim_panel() {
        let (s, _) = state::tests::bracket_clean();
        let mut h = harness_for(s);
        h.run();
        assert!(h.query_by_label("Play").is_none());
        h.get_by_label("View").click();
        h.run();
        menu(&h, "Simulation").click();
        h.run();
        assert!(h.state().state.sim_view.open);
        h.key_press(egui::Key::Escape);
        h.run();
        for name in ["Play", "Restart", "Speed"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        // the slider and its value field are both named "Time"
        assert_eq!(h.query_all_by_label("Time").count(), 2);
        assert!(h.query_by_label_contains("Total ").is_some());
        assert!(h.query_by_label_contains("File 1 · T1 · ").is_some());
        h.get_by_label("Play").click();
        h.run_steps(2); // playing keeps repainting, so run() would not settle
        assert!(h.state().state.sim_view.playing);
        assert!(h.query_by_label("Pause").is_some());
        h.get_by_label("Pause").click();
        h.run();
        assert!(!h.state().state.sim_view.playing);
        // closing the panel while playing pauses
        h.get_by_label("Play").click();
        h.run_steps(2);
        h.get_by_label("View").click();
        h.run_steps(2);
        menu(&h, "Simulation").click();
        h.run_steps(2);
        assert!(!h.state().state.sim_view.open);
        assert!(!h.state().state.sim_view.playing);
        h.key_press(egui::Key::Escape);
        h.run();
        h.state_mut().state.sim_view.open = true;

        // the first cutting feed step: the plunge's midpoint is still above Z 0
        let st = h
            .state()
            .state
            .sim
            .as_ref()
            .unwrap()
            .steps
            .iter()
            .find(|st| st.kind == hc_sim::StepKind::Feed && st.from.z < 0.0)
            .unwrap()
            .clone();
        h.state_mut().state.sim_view.t = (st.t0 + st.t1) / 2.0;
        h.run();
        use egui_kittest::kittest::NodeT;
        // a Label's text is its accesskit value, not its label
        let labels: Vec<String> = h
            .query_all_by_label_contains("mm/min")
            .filter_map(|n| n.accesskit_node().value())
            .collect();
        assert!(labels.iter().any(|l| l.contains("Z -")), "{labels:?}");

        let (mut s, _) = state::tests::bracket_issues();
        s.sim_view.open = true;
        let mut h = harness_for(s);
        h.run();
        assert!(h
            .query_by_label("Not simulated: 1 operation with errors")
            .is_some());
    }

    fn fresh() -> Harness<'static, App> {
        harness_for(AppState::new(None))
    }

    fn disabled(h: &Harness<'static, App>, label: &str) -> bool {
        use egui_kittest::kittest::NodeT;
        menu(h, label).accesskit_node().is_disabled()
    }

    fn open_menu(h: &mut Harness<'static, App>, name: &str) {
        h.get_by_label(name).click();
        h.run();
    }

    fn close_menu(h: &mut Harness<'static, App>) {
        h.key_press(egui::Key::Escape);
        h.run();
    }

    /// M10 test 21.
    #[test]
    fn m10_t21_edit_menu_and_undo_shortcuts() {
        let mut h = fresh();
        h.run();
        open_menu(&mut h, "Edit");
        assert!(disabled(&h, "Undo") && disabled(&h, "Redo"));
        close_menu(&mut h);

        open_menu(&mut h, "Draw");
        menu(&h, "Rectangle").click();
        h.run();
        h.get_by_label("Add").click();
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);
        open_menu(&mut h, "Edit");
        assert!(!disabled(&h, "Undo") && disabled(&h, "Redo"));
        close_menu(&mut h);

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert!(h.state().state.project.items.is_empty());
        h.key_press_modifiers(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert!(h.state().state.project.items.is_empty());
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Y);
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);

        // a focused text field keeps Cmd+Z
        h.get_by_label("Width").focus();
        h.run();
        assert!(h.ctx.egui_wants_keyboard_input());
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);
    }

    /// M10b test 30.
    #[test]
    fn m10_t30_text_menu_and_form() {
        let mut h = fresh();
        h.run();
        open_menu(&mut h, "Draw");
        menu(&h, "Text").click();
        h.run();
        assert_eq!(h.state().state.tool, state::CanvasTool::Text);
        assert!(h
            .query_by_label("Text: click the start of the baseline · Esc cancels")
            .is_some());
        for name in ["Text", "Font", "X", "Y", "Height", "Add"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        menu(&h, "Font").click();
        h.run();
        h.get_by_label("Outline (Ubuntu Light)").click();
        h.run();
        assert_eq!(h.state().state.draw.font, hc_text::Font::Outline);
        h.get_by_label("Add").click();
        h.run();
        let s = &h.state().state;
        assert!(!s.project.items.is_empty());
        assert_eq!(s.selection.len(), s.project.items.len());

        // T on the canvas
        h.state_mut().state.set_tool(state::CanvasTool::Select);
        h.hover_at(egui::pos2(640.0, 400.0));
        h.run();
        h.key_press(egui::Key::T);
        h.run();
        assert_eq!(h.state().state.tool, state::CanvasTool::Text);
    }

    /// M10 test 22.
    #[test]
    fn m10_t22_draw_menu_form_transform_stock() {
        let mut h = fresh();
        h.run();
        open_menu(&mut h, "Draw");
        for name in ["Select", "Line", "Rectangle", "Circle", "Set origin"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        menu(&h, "Set origin").click();
        h.run();
        assert_eq!(h.state().state.tool, state::CanvasTool::SetOrigin);
        assert!(h
            .query_by_label("Set origin: click a snap point · Esc cancels")
            .is_some());
        h.hover_at(egui::pos2(640.0, 400.0)); // the canvas
        h.run();
        h.key_press(egui::Key::Escape);
        h.run();
        assert_eq!(h.state().state.tool, state::CanvasTool::Select);

        // Line form
        open_menu(&mut h, "Draw");
        menu(&h, "Line").click();
        h.run();
        for name in ["X1", "Y1", "X2", "Y2", "Add"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        h.get_by_label("X2").set_value_accesskit(12.0);
        h.run();
        h.get_by_label("Y2").set_value_accesskit(5.0);
        h.run();
        h.get_by_label("Add").click();
        h.run();
        let s = &h.state().state;
        assert_eq!(s.project.items.len(), 1);
        assert_eq!(s.item_bbox[0].max, hc_geom::Point::new(12.0, 5.0));

        // one item selected: transform group
        open_menu(&mut h, "Draw");
        menu(&h, "Select").click();
        h.run();
        for name in [
            "Move",
            "Rotate",
            "Scale",
            "Flip horizontally",
            "Flip vertically",
            "Delete",
        ] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        h.get_by_label("Delete").click();
        h.run();
        assert!(h.state().state.project.items.is_empty());

        // Settings > Stock
        open_menu(&mut h, "Settings");
        menu(&h, "Stock").hover();
        h.run();
        for name in ["Thickness", "Picked point", "Pick on drawing"] {
            assert!(menu_q(&h, name, false).is_some(), "{name}");
        }
        h.get_by_label("Stock center").click();
        h.run();
        assert_eq!(
            h.state().state.project.stock.origin,
            hc_project::WorkOrigin::StockCenter
        );
    }

    fn toggled(n: &egui_kittest::Node<'_>) -> bool {
        use egui_kittest::kittest::NodeT;
        n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    }

    fn side_disabled(h: &Harness<'static, App>, name: &str) -> bool {
        use egui_kittest::kittest::NodeT;
        side(h, name).accesskit_node().is_disabled()
    }

    const SIDEBAR: [&str; 23] = [
        "New",
        "Open project…",
        "Save",
        "Import DXF…",
        "Export G-code…",
        "Undo",
        "Redo",
        "Delete selection",
        "Rotate selection",
        "Flip selection horizontally",
        "Flip selection vertically",
        "Select",
        "Line",
        "Rectangle",
        "Circle",
        "Set origin",
        "Zoom to fit",
        "Show toolpaths",
        "Simulation",
        "Units",
        "Machine",
        "Stock",
        "Tool library…",
    ];

    /// Clicks a sidebar button after moving the pointer away, so the previous button's tooltip cannot cover it.
    fn click_side(h: &mut Harness<'static, App>, name: &str) {
        h.hover_at(egui::pos2(700.0, 400.0));
        h.run();
        side(h, name).scroll_to_me();
        h.run();
        side(h, name).click();
        h.run();
    }

    /// M10a.2 test 12. Save and Import DXF open a file dialog, so they are only checked for presence (as the menu tests).
    #[test]
    fn m10a2_t12_sidebar() {
        let (s, _) = state::tests::bracket_issues();
        let mut h = harness_for(s);
        h.run();
        for name in SIDEBAR {
            side(&h, name);
        }
        assert!(h.state().state.dirty);
        click_side(&mut h, "New");
        assert_eq!(h.state().pending, Some(Pending::New));
        h.state_mut().pending = None;
        h.run();
        click_side(&mut h, "Open project…");
        assert_eq!(h.state().pending, Some(Pending::Open));
        h.state_mut().pending = None;
        h.run();
        click_side(&mut h, "Export G-code…");
        assert_eq!(h.state().export_prompt, Some(hc_toolpath::Severity::Error));
        h.state_mut().export_prompt = None;
        h.run();

        // fresh project: edit buttons
        let mut h = fresh();
        h.run();
        assert!(side_disabled(&h, "Undo") && side_disabled(&h, "Redo"));
        for name in [
            "Delete selection",
            "Rotate selection",
            "Flip selection horizontally",
            "Flip selection vertically",
        ] {
            assert!(side_disabled(&h, name), "{name}");
        }
        h.state_mut()
            .state
            .add_shape(state::Shape::Rect {
                min: hc_geom::Point::new(0.0, 0.0),
                w: 20.0,
                h: 10.0,
                r: 0.0,
            })
            .unwrap();
        h.run();
        assert!(!side_disabled(&h, "Undo"));
        let bbox = |h: &Harness<'static, App>| h.state().state.selection_bbox();
        click_side(&mut h, "Rotate selection");
        assert!((bbox(&h).width() - 10.0).abs() < 1e-9); // 90° default
        let before = h.state().state.project.clone();
        click_side(&mut h, "Flip selection horizontally");
        assert_ne!(h.state().state.project, before);
        click_side(&mut h, "Flip selection vertically");
        click_side(&mut h, "Undo");
        assert!(!side_disabled(&h, "Redo"));
        click_side(&mut h, "Redo");
        click_side(&mut h, "Delete selection");
        assert!(h.state().state.project.items.is_empty());

        // draw tools: exactly one selected
        let tools = [
            ("Select", state::CanvasTool::Select),
            ("Line", state::CanvasTool::Line),
            ("Rectangle", state::CanvasTool::Rect),
            ("Circle", state::CanvasTool::Circle),
            ("Text", state::CanvasTool::Text),
            ("Set origin", state::CanvasTool::SetOrigin),
        ];
        for (name, tool) in tools.iter().rev() {
            click_side(&mut h, name);
            assert_eq!(h.state().state.tool, *tool);
            for (other, _) in tools {
                assert_eq!(toggled(&side(&h, other)), other == *name, "{name}/{other}");
            }
        }

        // view
        h.state_mut().state.fit_pending = false;
        click_side(&mut h, "Zoom to fit");
        // the canvas consumes fit_pending in the same frame: the view was fitted (zoom changes from 1.0)
        assert!(!h.state().state.fit_pending);
        assert!(toggled(&side(&h, "Show toolpaths")));
        click_side(&mut h, "Show toolpaths");
        assert!(!h.state().state.show_toolpaths);
        assert!(!toggled(&side(&h, "Show toolpaths")));
        click_side(&mut h, "Simulation");
        assert!(h.state().state.sim_view.open && toggled(&side(&h, "Simulation")));

        // settings popups: the same fields as the menu
        for (name, field) in [
            ("Units", "mm"),
            ("Machine", "Safe Z"),
            ("Stock", "Thickness"),
        ] {
            click_side(&mut h, name);
            assert!(menu_q(&h, field, false).is_some(), "{name}: {field}");
            h.key_press(egui::Key::Escape);
            h.run();
            assert!(menu_q(&h, field, false).is_none(), "{name} closed");
        }
        click_side(&mut h, "Tool library…");
        assert!(h.state().tool_dialog.open);
    }

    /// M10a.2 test 14.
    #[test]
    fn m10a2_t14_capped_hint() {
        let mut s = AppState::new(None);
        let lines: Vec<hc_project::Geom> = (0..snap::MAX_X_SEGMENTS + 1)
            .map(|k| {
                let x = k as f64;
                hc_project::Geom::Contour(
                    hc_geom::shapes::line(hc_geom::Point::new(x, 0.0), hc_geom::Point::new(x, 1.0))
                        .unwrap(),
                )
            })
            .collect();
        s.project.add_drawn(lines);
        s.items_changed();
        s.set_tool(state::CanvasTool::Line);
        let mut h = harness_for(s);
        h.run();
        assert!(h.state().state.snap_cache.capped);
        use egui_kittest::kittest::NodeT;
        let label = h
            .query_all_by_label_contains("Esc cancels")
            .filter_map(|n| n.accesskit_node().label().or(n.accesskit_node().value()))
            .next()
            .expect("hint");
        assert!(
            label.ends_with("Intersection snaps off: drawing too large"),
            "{label}"
        );
    }

    /// Orchestrator decision (M10a.2): at 1280 x 800 every sidebar button is visible without scrolling.
    #[test]
    fn m10a2_sidebar_fits() {
        let (s, _) = state::tests::bracket_issues(); // messages, issues and status panels all shown
        let mut h = harness_for(s);
        h.run();
        let status_top = h.get_by_label("X – Y –").rect().min.y;
        let issues_top = h.get_by_label_contains("Issues (").rect().min.y;
        let bottom = status_top.min(issues_top);
        let mut xs = std::collections::BTreeSet::new();
        for name in SIDEBAR {
            let r = side(&h, name).rect();
            assert!(r.max.y <= bottom, "{name} at {r:?}, panel bottom {bottom}");
            assert!(r.min.y >= 0.0);
            xs.insert(r.min.x as i32);
        }
        assert_eq!(xs.len(), 2, "two columns: {xs:?}");
    }
}
