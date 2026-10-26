//! Tool library window: list, add/edit/delete, save.

use crate::panels::{fmt_len, length_field, Quantity};
use crate::App;
use eframe::egui;
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
        .default_width(480.0)
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
    egui::Grid::new("tool_form").num_columns(2).show(ui, |ui| {
        ui.label("Number");
        ui.add_enabled(
            *is_new,
            egui::DragValue::new(&mut t.id).range(1..=9999).prefix("T"),
        );
        ui.end_row();
        ui.label("Name");
        ui.text_edit_singleline(&mut t.name);
        ui.end_row();
        ui.label("Type");
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
            });
        ui.end_row();
        if let ToolKind::VBit { angle_deg } = &mut t.kind {
            ui.label("V angle");
            ui.add(
                egui::DragValue::new(angle_deg)
                    .range(1.0..=179.0)
                    .suffix("°"),
            );
            ui.end_row();
        }
        ui.label("Diameter");
        length_field(ui, &mut t.diameter, units, Quantity::Length);
        ui.end_row();
        ui.label("Flute length");
        length_field(ui, &mut t.flute_length, units, Quantity::Length);
        ui.end_row();
        ui.label("Flutes");
        ui.add(egui::DragValue::new(&mut t.flutes).range(1..=12));
        ui.end_row();
        ui.label("Feed");
        length_field(ui, &mut t.feed, units, Quantity::Feed);
        ui.end_row();
        ui.label("Plunge feed");
        length_field(ui, &mut t.plunge_feed, units, Quantity::Feed);
        ui.end_row();
        ui.label("RPM");
        ui.add(egui::DragValue::new(&mut t.rpm).range(0..=100_000));
        ui.end_row();
        ui.label("Depth/pass");
        length_field(ui, &mut t.depth_per_pass, units, Quantity::Length);
        ui.end_row();
        ui.label("Stepover");
        ui.add(
            egui::DragValue::new(&mut t.stepover_pct)
                .range(1.0..=100.0)
                .suffix(" %"),
        );
        ui.end_row();
        ui.label("Comment");
        ui.text_edit_singleline(&mut t.comment);
        ui.end_row();
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
