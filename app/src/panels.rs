//! Menu, side panels, message panel and status bar. All edits go through `AppState`.

use crate::state::{circle_diameter, kind_label, MoveDir, NewKind, Severity};
use crate::{aci, App, Pending};
use eframe::egui::{self, KeyboardShortcut, Modifiers, Ui};
use hc_project::{from_display, parse_length, to_display, Direction, Geom, OpKind, Side, Units};

#[derive(Clone, Copy)]
pub enum Quantity {
    Length,
    Feed,
}

/// Length/feed `DragValue`: shows display units, accepts `1/8`, `3 mm`, …, writes mm back.
pub fn length_field(ui: &mut Ui, mm: &mut f64, units: Units, q: Quantity) -> egui::Response {
    let mut v = to_display(*mm, units);
    let suffix = match (q, units) {
        (Quantity::Length, Units::Mm) => " mm",
        (Quantity::Length, Units::Inch) => " in",
        (Quantity::Feed, Units::Mm) => " mm/min",
        (Quantity::Feed, Units::Inch) => " in/min",
    };
    let r = ui.add(
        egui::DragValue::new(&mut v)
            .speed(if units == Units::Mm { 0.1 } else { 0.005 })
            .max_decimals(if units == Units::Mm { 3 } else { 4 })
            .custom_parser(move |s| parse_length(s, units).map(|mm| to_display(mm, units)))
            .suffix(suffix),
    );
    if r.changed() {
        *mm = from_display(v, units);
    }
    r
}

/// Read-only length in display units: 3 decimals for mm, 4 for inch.
pub fn fmt_len(mm: f64, u: Units) -> String {
    match u {
        Units::Mm => format!("{mm:.3} mm"),
        Units::Inch => format!("{:.4} in", to_display(mm, u)),
    }
}

const SC_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::N);
const SC_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O);
const SC_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::S);
const SC_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), egui::Key::S);
const SC_IMPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::I);

pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    // Shift+S first: consume_shortcut ignores extra modifiers
    if ctx.input_mut(|i| i.consume_shortcut(&SC_SAVE_AS)) {
        app.save_as();
    } else if ctx.input_mut(|i| i.consume_shortcut(&SC_SAVE)) {
        app.save();
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SC_NEW)) {
        app.request(ctx, Pending::New);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SC_OPEN)) {
        app.request(ctx, Pending::Open);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SC_IMPORT)) {
        app.import_dxf();
    }
}

fn item(ui: &mut Ui, text: &str, sc: Option<&KeyboardShortcut>) -> bool {
    let mut b = egui::Button::new(text);
    if let Some(sc) = sc {
        b = b.shortcut_text(ui.ctx().format_shortcut(sc));
    }
    ui.add(b).clicked()
}

pub fn menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if item(ui, "New", Some(&SC_NEW)) {
                    app.request(&ctx, Pending::New);
                }
                if item(ui, "Open project…", Some(&SC_OPEN)) {
                    app.request(&ctx, Pending::Open);
                }
                if item(ui, "Save", Some(&SC_SAVE)) {
                    app.save();
                }
                if item(ui, "Save as…", Some(&SC_SAVE_AS)) {
                    app.save_as();
                }
                ui.separator();
                if item(ui, "Import DXF…", Some(&SC_IMPORT)) {
                    app.import_dxf();
                }
                ui.separator();
                if item(ui, "Quit", None) {
                    app.request(&ctx, Pending::Quit);
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Zoom to fit").shortcut_text("F"))
                    .clicked()
                {
                    app.state.fit_pending = true;
                }
            });
            ui.menu_button("Settings", |ui| {
                ui.menu_button("Units", |ui| {
                    let u = app.state.project.units;
                    if ui.radio(u == Units::Mm, "mm").clicked() {
                        app.state.set_units(Units::Mm);
                    }
                    if ui.radio(u == Units::Inch, "inch").clicked() {
                        app.state.set_units(Units::Inch);
                    }
                });
                if ui.button("Tool library…").clicked() {
                    app.tool_dialog.open = true;
                }
            });
        });
    });
}

pub fn messages(app: &mut App, ui: &mut Ui) {
    if app.state.messages.is_empty() {
        return;
    }
    egui::Panel::top("messages").show(ui, |ui| {
        let mut remove = None;
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .show(ui, |ui| {
                for (i, m) in app.state.messages.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("Dismiss").clicked() {
                            remove = Some(i);
                        }
                        let (tag, color) = match m.severity {
                            Severity::Info => ("Info", ui.visuals().text_color()),
                            Severity::Warning => ("Warning", ui.visuals().warn_fg_color),
                            Severity::Error => ("Error", ui.visuals().error_fg_color),
                        };
                        ui.colored_label(color, tag);
                        ui.label(&m.text);
                    });
                }
            });
        if let Some(i) = remove {
            app.state.messages.remove(i);
        }
    });
}

pub fn status_bar(app: &mut App, ui: &mut Ui) {
    let s = &app.state;
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let u = s.project.units;
            let (name, dec) = match u {
                Units::Mm => ("mm", 3),
                Units::Inch => ("in", 4),
            };
            match s.cursor {
                Some(p) => ui.monospace(format!(
                    "X {:.dec$} Y {:.dec$} {name}",
                    to_display(p.x, u),
                    to_display(p.y, u)
                )),
                None => ui.monospace("X – Y –"),
            };
            ui.separator();
            ui.label(format!("Zoom {:.0}%", s.view.zoom * 100.0));
            ui.separator();
            ui.label(format!("{} selected", s.selection.len()));
        });
    });
}

pub fn left(app: &mut App, ui: &mut Ui) {
    egui::Panel::left("left")
        .default_size(260.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                layers(app, ui);
                ui.separator();
                operations(app, ui);
            });
        });
}

fn layers(app: &mut App, ui: &mut Ui) {
    ui.heading("Layers");
    let dark = ui.visuals().dark_mode;
    let layers = app.state.project.layers.clone();
    if layers.is_empty() {
        ui.weak("No layers. Use File > Import DXF…");
    }
    for l in &layers {
        ui.horizontal(|ui| {
            let mut vis = !app.state.hidden_layers.contains(&l.name);
            let r = ui.checkbox(&mut vis, "");
            let label = format!("Show layer {}", l.name);
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, vis, &label));
            if r.changed() {
                app.state.set_layer_visible(&l.name, vis);
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 2.0, aci::aci_rgb(l.color, dark));
            ui.label(&l.name);
            ui.weak(format!("({})", app.state.layer_item_count(&l.name)));
        });
    }
}

fn operations(app: &mut App, ui: &mut Ui) {
    ui.heading("Operations");
    enum Act {
        Select(u64),
        Move(u64, MoveDir),
        Delete(u64),
        Enable(u64, bool),
    }
    let mut act = None;
    for op in &app.state.project.operations {
        ui.horizontal(|ui| {
            let mut en = op.enabled;
            if ui.checkbox(&mut en, "").changed() {
                act = Some(Act::Enable(op.id, en));
            }
            let text = format!("{}  {}  T{}", op.name, kind_label(&op.kind), op.tool);
            if ui
                .selectable_label(app.state.selected_op == Some(op.id), text)
                .clicked()
            {
                act = Some(Act::Select(op.id));
            }
            if ui.small_button("Up").clicked() {
                act = Some(Act::Move(op.id, MoveDir::Up));
            }
            if ui.small_button("Down").clicked() {
                act = Some(Act::Move(op.id, MoveDir::Down));
            }
            if ui.small_button("Delete").clicked() {
                act = Some(Act::Delete(op.id));
            }
        });
    }
    match act {
        Some(Act::Select(id)) => app.state.select_op(id),
        Some(Act::Move(id, d)) => app.state.move_op(id, d),
        Some(Act::Delete(id)) => app.state.delete_op(id),
        Some(Act::Enable(id, en)) => {
            if let Some(mut op) = app
                .state
                .project
                .operations
                .iter()
                .find(|o| o.id == id)
                .cloned()
            {
                op.enabled = en;
                app.state.update_op(op);
            }
        }
        None => {}
    }

    let tools = app.state.available_tools();
    match app.new_op {
        None => {
            let r = ui.add_enabled(
                !app.state.selection.is_empty(),
                egui::Button::new("New operation"),
            );
            if r.on_disabled_hover_text("Select geometry first").clicked() {
                app.new_op = Some((NewKind::Pocket, tools.first().map_or(0, |t| t.id)));
            }
        }
        Some((mut kind, mut tool)) => {
            app.new_op = Some((kind, tool));
            ui.group(|ui| {
                egui::ComboBox::from_label("Kind")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for k in NewKind::ALL {
                            ui.selectable_value(&mut kind, k, k.label());
                        }
                    });
                tool_combo(ui, "new_op_tool", &mut tool, &tools);
                app.new_op = Some((kind, tool));
                if tools.is_empty() {
                    ui.weak("No tools: add one in Settings > Tool library…");
                }
                let (k, t) = (kind, tool);
                ui.horizontal(|ui| {
                    let ok = !app.state.selection.is_empty() && tools.iter().any(|x| x.id == t);
                    if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
                        app.state.new_operation(k, t);
                        app.new_op = None;
                    }
                    if ui.button("Cancel").clicked() {
                        app.new_op = None;
                    }
                });
            });
        }
    }
}

fn tool_combo(ui: &mut Ui, id: &str, tool: &mut u32, tools: &[hc_project::Tool]) {
    let text = tools
        .iter()
        .find(|t| t.id == *tool)
        .map_or(format!("T{tool} (missing)"), |t| {
            format!("T{} {}", t.id, t.name)
        });
    ui.horizontal(|ui| {
        ui.label("Tool");
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .show_ui(ui, |ui| {
                for t in tools {
                    ui.selectable_value(tool, t.id, format!("T{} {}", t.id, t.name));
                }
            });
    });
}

pub fn right(app: &mut App, ui: &mut Ui) {
    egui::Panel::right("props")
        .default_size(260.0)
        .show(ui, |ui| {
            ui.heading("Properties");
            egui::ScrollArea::vertical().show(ui, |ui| {
                let op = app
                    .state
                    .selected_op
                    .and_then(|id| app.state.project.operations.iter().find(|o| o.id == id))
                    .cloned();
                match op {
                    Some(op) => op_props(app, ui, op),
                    None => selection_props(app, ui),
                }
            });
        });
}

fn op_props(app: &mut App, ui: &mut Ui, mut op: hc_project::Operation) {
    let u = app.state.project.units;
    let tools = app.state.available_tools();
    let (mut defaults, mut use_sel) = (false, false);
    egui::Grid::new("op_props").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut op.name);
        ui.end_row();
        ui.label("Enabled");
        ui.checkbox(&mut op.enabled, "");
        ui.end_row();
        ui.label("Kind");
        ui.label(kind_label(&op.kind));
        ui.end_row();
        match &mut op.kind {
            OpKind::Engrave => {}
            OpKind::Contour { side, direction } => {
                ui.label("Side");
                egui::ComboBox::from_id_salt("side")
                    .selected_text(format!("{side:?}"))
                    .show_ui(ui, |ui| {
                        for s in [Side::Outside, Side::Inside, Side::On] {
                            ui.selectable_value(side, s, format!("{s:?}"));
                        }
                    });
                ui.end_row();
                ui.label("Direction");
                egui::ComboBox::from_id_salt("dir")
                    .selected_text(format!("{direction:?}"))
                    .show_ui(ui, |ui| {
                        for d in [Direction::Climb, Direction::Conventional] {
                            ui.selectable_value(direction, d, format!("{d:?}"));
                        }
                    });
                ui.end_row();
            }
            OpKind::Pocket { stepover_pct } => {
                ui.label("Stepover");
                ui.add(
                    egui::DragValue::new(stepover_pct)
                        .range(1.0..=100.0)
                        .suffix(" %"),
                );
                ui.end_row();
            }
            OpKind::Drill { peck } => {
                ui.label("Peck");
                ui.horizontal(|ui| {
                    let mut on = peck.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        *peck = on.then_some(op.depth_per_pass);
                    }
                    if let Some(p) = peck {
                        length_field(ui, p, u, Quantity::Length);
                    }
                });
                ui.end_row();
            }
        }
        ui.label("Depth");
        length_field(ui, &mut op.depth, u, Quantity::Length);
        ui.end_row();
        ui.label("Depth/pass");
        length_field(ui, &mut op.depth_per_pass, u, Quantity::Length);
        ui.end_row();
        ui.label("Feed");
        length_field(ui, &mut op.feed, u, Quantity::Feed);
        ui.end_row();
        ui.label("Plunge feed");
        length_field(ui, &mut op.plunge_feed, u, Quantity::Feed);
        ui.end_row();
        ui.label("RPM");
        ui.add(egui::DragValue::new(&mut op.rpm).range(0..=100_000));
        ui.end_row();
    });
    tool_combo(ui, "op_tool", &mut op.tool, &tools);
    ui.label(format!("{} items", op.items.len()));
    ui.horizontal(|ui| {
        defaults = ui.button("Apply tool defaults").clicked();
        use_sel = ui.button("Use current selection").clicked();
    });
    let id = op.id;
    app.state.update_op(op);
    if defaults {
        app.state.apply_tool_defaults(id);
    }
    if use_sel {
        app.state.use_selection(id);
    }
}

fn selection_props(app: &mut App, ui: &mut Ui) {
    let s = &app.state;
    let u = s.project.units;
    let len = |mm: f64| fmt_len(mm, u);
    ui.label(format!("{} selected", s.selection.len()));
    if s.selection.len() != 1 {
        return;
    }
    let Some(it) = s.item(*s.selection.first().unwrap()) else {
        return;
    };
    egui::Grid::new("item_props").num_columns(2).show(ui, |ui| {
        ui.label("Layer");
        ui.label(&it.layer);
        ui.end_row();
        match &it.geom {
            Geom::Point(p) => {
                ui.label("Point");
                ui.label(format!("{}, {}", len(p.x), len(p.y)));
                ui.end_row();
            }
            Geom::Contour(c) => {
                ui.label("Shape");
                ui.label(if c.closed { "closed" } else { "open" });
                ui.end_row();
                ui.label("Segments");
                ui.label(c.segments.len().to_string());
                ui.end_row();
                ui.label("Length");
                ui.label(len(c.length()));
                ui.end_row();
                let b = c.bbox();
                ui.label("Size");
                ui.label(format!("{} × {}", len(b.width()), len(b.height())));
                ui.end_row();
                if let Some(d) = circle_diameter(it) {
                    ui.label("Ø");
                    ui.label(len(d));
                    ui.end_row();
                }
            }
        }
    });
}
