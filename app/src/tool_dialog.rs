//! Tool library window: list, add/edit/delete, save.

use crate::panels::{length_field, Quantity};
use crate::App;
use eframe::egui;
use hc_project::fmt_len;
use hc_project::{Tool, ToolKind};

#[derive(Default)]
pub struct ToolDialog {
    pub open: bool,
    /// Tool being edited and whether it is new.
    edit: Option<(Tool, bool)>,
}

fn kind_name(k: &ToolKind) -> &'static str {
    match k {
        ToolKind::FlatEndMill => "Flat end mill",
        ToolKind::BallEndMill => "Ball end mill",
        ToolKind::VBit { .. } => "V-bit",
        ToolKind::Drill => "Drill",
    }
}

fn new_tool(id: u32) -> Tool {
    Tool {
        id,
        name: format!("Tool {id}"),
        kind: ToolKind::FlatEndMill,
        diameter: 3.0,
        flute_length: 12.0,
        flutes: 2,
        feed: 600.0,
        plunge_feed: 150.0,
        rpm: 16000,
        depth_per_pass: 1.0,
        stepover_pct: 40.0,
        comment: String::new(),
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let mut open = app.tool_dialog.open;
    if !open {
        return;
    }
    egui::Window::new("Tool library")
        .open(&mut open)
        .default_width(640.0)
        .show(ctx, |ui| body(app, ui));
    app.tool_dialog.open = open;
}

fn body(app: &mut App, ui: &mut egui::Ui) {
    let units = app.state.project.units;
    let d = &mut app.tool_dialog;
    let s = &mut app.state;
    if let Some(err) = &s.library_error {
        ui.colored_label(ui.visuals().error_fg_color, err);
        if s.library_path.is_some() && ui.button("Start a new empty library").clicked() {
            s.start_new_library();
        }
        return;
    }

    let mut delete = None;
    egui::Grid::new("tool_list").striped(true).show(ui, |ui| {
        for t in &s.library.tools {
            ui.label(format!("T{}", t.id));
            ui.label(&t.name);
            ui.label(kind_name(&t.kind));
            ui.label(format!("Ø {}", fmt_len(t.diameter, units)));
            if ui.small_button("Edit").clicked() {
                d.edit = Some((t.clone(), false));
            }
            if ui.small_button("Delete").clicked() {
                delete = Some(t.id);
            }
            ui.end_row();
        }
    });
    if let Some(id) = delete {
        s.library_remove(id);
        if d.edit.as_ref().is_some_and(|(t, _)| t.id == id) {
            d.edit = None;
        }
    }
    if ui.button("Add").clicked() {
        d.edit = Some((new_tool(s.library.next_free_id()), true));
    }
    ui.weak("Changes are saved at once. They do not change tools already used in the project.");

    let Some((t, is_new)) = d.edit.as_mut() else {
        return;
    };
    ui.separator();
    let mut done = None;
    ui.horizontal_top(|ui| {
        egui::Grid::new("tool_form").num_columns(2).show(ui, |ui| {
            let l = ui.label("Number");
            ui.add_enabled(
                *is_new,
                egui::DragValue::new(&mut t.id).range(1..=9999).prefix("T"),
            )
            .labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Name");
            ui.text_edit_singleline(&mut t.name).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Type");
            egui::ComboBox::from_id_salt("tool_kind")
                .selected_text(kind_name(&t.kind))
                .show_ui(ui, |ui| {
                    for k in [
                        ToolKind::FlatEndMill,
                        ToolKind::BallEndMill,
                        ToolKind::VBit { angle_deg: 60.0 },
                        ToolKind::Drill,
                    ] {
                        let sel = kind_name(&t.kind) == kind_name(&k);
                        if ui.selectable_label(sel, kind_name(&k)).clicked() && !sel {
                            t.kind = k;
                        }
                    }
                })
                .response
                .labelled_by(l.id);
            ui.end_row();
            if let ToolKind::VBit { angle_deg } = &mut t.kind {
                let l = ui.label("V angle");
                ui.add(
                    egui::DragValue::new(angle_deg)
                        .range(1.0..=179.0)
                        .suffix("°"),
                )
                .labelled_by(l.id);
                ui.end_row();
            }
            let l = ui.label("Diameter");
            length_field(ui, &mut t.diameter, units, Quantity::Length).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Flute length");
            length_field(ui, &mut t.flute_length, units, Quantity::Length).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Flutes");
            ui.add(egui::DragValue::new(&mut t.flutes).range(1..=12))
                .labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Feed");
            length_field(ui, &mut t.feed, units, Quantity::Feed).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Plunge feed");
            length_field(ui, &mut t.plunge_feed, units, Quantity::Feed).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("RPM");
            ui.add(egui::DragValue::new(&mut t.rpm).range(0..=100_000))
                .labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Depth/pass");
            length_field(ui, &mut t.depth_per_pass, units, Quantity::Length).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Stepover");
            ui.add(
                egui::DragValue::new(&mut t.stepover_pct)
                    .range(1.0..=100.0)
                    .suffix(" %"),
            )
            .labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Comment");
            ui.text_edit_singleline(&mut t.comment).labelled_by(l.id);
            ui.end_row();
        });
        ui.add_space(12.0);
        crate::tool_sketch::paint(ui, &crate::tool_sketch::sketch(t, units));
    });
    ui.horizontal(|ui| {
        if ui.button("OK").clicked() {
            done = Some(true);
        }
        if ui.button("Cancel").clicked() {
            done = Some(false);
        }
    });
    match done {
        Some(true) => {
            let (t, is_new) = d.edit.clone().unwrap();
            if is_new {
                s.library_add(t.clone());
            } else {
                s.library_update(t.clone());
            }
            // close once the edit is in the library (even if the save failed: it stays in memory);
            // a rejected edit (e.g. duplicate number) keeps the form open
            if s.library.get(t.id) == Some(&t) {
                d.edit = None;
            }
        }
        Some(false) => d.edit = None,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    fn harness(size: egui::Vec2, theme: egui::Theme, tools: bool) -> Harness<'static, App> {
        let mut s = if tools {
            crate::state::tests::bracket_clean().0
        } else {
            AppState::new(None)
        };
        s.library_error = None;
        let mut h = Harness::builder()
            .with_size(size)
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.state_mut().tool_dialog.open = true;
        h.run();
        h
    }
    fn sketch_label(h: &Harness<'_, App>) -> Option<String> {
        h.query_all_by_label_contains("Tool sketch")
            .next()
            .map(|n| n.accesskit_node().label().unwrap_or_default())
    }
    fn edit_with(h: &mut Harness<'_, App>, t: Tool) {
        h.state_mut().tool_dialog.edit = Some((t, false));
        h.run();
    }
    fn vbit(a: f64) -> Tool {
        let mut t = new_tool(1);
        t.kind = ToolKind::VBit { angle_deg: a };
        t
    }

    #[test]
    fn x6_no_sketch_without_edit_or_tools() {
        for tools in [true, false] {
            let mut h = harness(egui::vec2(1280.0, 800.0), egui::Theme::Light, tools);
            assert!(sketch_label(&h).is_none());
            // empty library: Add still gives a form and a sketch
            h.get_by_label("Add").click();
            h.run();
            assert!(sketch_label(&h).unwrap().contains("Flutes 2"));
            h.get_by_label("Cancel").click();
            h.run();
            assert!(sketch_label(&h).is_none());
        }
        let mut h = harness(egui::vec2(1280.0, 800.0), egui::Theme::Light, true);
        h.state_mut().state.library_error = Some("broken".into());
        h.run();
        assert!(sketch_label(&h).is_none());
    }

    #[test]
    fn x7_live_update_and_all_kinds() {
        let mut h = harness(egui::vec2(1280.0, 800.0), egui::Theme::Light, true);
        edit_with(&mut h, new_tool(1));
        assert!(sketch_label(&h).unwrap().contains("Diameter 3.000 mm"));
        // change Diameter in the form by typing
        h.get_by_label("Diameter").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run();
        h.event(egui::Event::Text("7.5".into()));
        h.key_press(egui::Key::Enter);
        h.run();
        h.run();
        let l = sketch_label(&h).unwrap();
        assert!(l.contains("Diameter 7.500 mm"), "{l}");
        // Type: each kind in turn
        for (name, v) in [
            ("Ball end mill", false),
            ("V-bit", true),
            ("Drill", false),
            ("Flat end mill", false),
        ] {
            h.get_by_label("Type").click();
            h.run();
            // list rows share the kind names; the popup entry is the last node
            h.query_all_by_label(name).last().unwrap().click();
            h.run();
            let l = sketch_label(&h).unwrap();
            assert_eq!(l.contains("V angle"), v, "{name}: {l}");
            assert!(l.contains("Diameter 7.500 mm"), "{l}");
        }
        // direct state change shows within one frame
        edit_with(&mut h, vbit(33.0));
        assert!(sketch_label(&h).unwrap().contains("V angle 33°"));
    }

    #[test]
    fn x8_v170_fits_and_dialog_fits_screen() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = harness(egui::vec2(1280.0, 800.0), theme, true);
            edit_with(&mut h, vbit(170.0));
            let r = h.get_by_label_contains("Tool sketch").rect();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
            assert!(screen.contains_rect(r), "{r:?}");
            assert!(sketch_label(&h).unwrap().contains("V angle 170°"));
            // the window (OK / Cancel row and title) is on screen too
            for n in ["OK", "Cancel", "Tool library"] {
                let r = h.get_by_label(n).rect();
                assert!(screen.contains_rect(r), "{n} {r:?}");
            }
            // the sketch sits right of the form
            let form = h.get_by_label("Comment").rect();
            let sk = h.get_by_label_contains("Tool sketch").rect();
            assert!(sk.min.x >= form.max.x, "{form:?} {sk:?}");
        }
    }
}
