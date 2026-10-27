//! Menu, side panels, message panel and status bar. All edits go through `AppState`.

use crate::state::{kind_label, MoveDir, NewKind, Severity};
use crate::{aci, App, Pending};
use eframe::egui::{self, KeyboardShortcut, Modifiers, Ui};
use hc_project::{
    fmt_len, from_display, parse_length, to_display, Direction, Geom, OpKind, Side, TabPlacement,
    Units,
};

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

const SC_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::N);
const SC_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O);
const SC_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::S);
const SC_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), egui::Key::S);
const SC_IMPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::I);
const SC_EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::E);

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
    if ctx.input_mut(|i| i.consume_shortcut(&SC_EXPORT)) {
        app.export_gcode();
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
                if item(ui, "Export G-code…", Some(&SC_EXPORT)) {
                    app.export_gcode();
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
                ui.checkbox(&mut app.state.show_toolpaths, "Show toolpaths");
                ui.checkbox(&mut app.state.sim_view.open, "Simulation");
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
                ui.menu_button("Machine", |ui| {
                    let u = app.state.project.units;
                    let mut m = app.state.project.machine.clone();
                    egui::Grid::new("machine").show(ui, |ui| {
                        for (name, v) in [
                            ("Safe Z", &mut m.safe_z),
                            ("Clearance Z", &mut m.clearance_z),
                            ("Tool change Z", &mut m.tool_change_z),
                        ] {
                            let l = ui.label(name);
                            length_field(ui, v, u, Quantity::Length).labelled_by(l.id);
                            ui.end_row();
                        }
                    });
                    app.state.set_machine(m);
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

const SPEEDS: [f64; 7] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];

/// Simulation controls, readout and time per file; sits directly above the status bar.
pub fn sim_panel(app: &mut App, ui: &mut Ui) {
    if !app.state.sim_view.open {
        app.state.sim_view.playing = false; // no invisible animation burning CPU
        return;
    }
    egui::Panel::bottom("sim").show(ui, |ui| {
        let s = &mut app.state;
        let sim = match &s.sim {
            Ok(sim) => sim,
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                return;
            }
        };
        let skipped = sim.skipped.len();
        let warn_skipped = |ui: &mut Ui| {
            if skipped > 0 {
                let plural = if skipped == 1 { "" } else { "s" };
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Not simulated: {skipped} operation{plural} with errors"),
                );
            }
        };
        if sim.steps.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Nothing to simulate");
                warn_skipped(ui);
            });
            return;
        }
        let total = sim.total();
        let u = s.project.units;
        let v = &mut s.sim_view;
        ui.horizontal(|ui| {
            let play = if v.playing { "Pause" } else { "Play" };
            let play_clicked = ui.button(play).clicked();
            if ui.button("Restart").clicked() {
                v.t = 0.0;
            }
            let l = ui.label("Speed");
            egui::ComboBox::from_id_salt("sim_speed")
                .selected_text(format!("{}×", v.speed))
                .show_ui(ui, |ui| {
                    for sp in SPEEDS {
                        ui.selectable_value(&mut v.speed, sp, format!("{sp}×"));
                    }
                })
                .response
                .labelled_by(l.id);
            let l = ui.label("Time");
            let r = ui
                .add(
                    egui::Slider::new(&mut v.t, 0.0..=total)
                        .custom_formatter(|t, _| hc_sim::fmt_duration(t)),
                )
                .labelled_by(l.id);
            if r.dragged() || r.changed() {
                v.playing = false;
            }
            if play_clicked {
                v.toggle(total);
            }
            let t = v.t;
            let Some(i) = sim.step_at(t) else { return };
            let st = &sim.steps[i];
            let op = s
                .project
                .operations
                .iter()
                .find(|o| o.id == st.op)
                .map_or(String::new(), |o| o.name.clone());
            let tool = s
                .project
                .tools
                .iter()
                .find(|x| x.id == st.tool)
                .map_or(format!("T{}", st.tool), |x| format!("T{} {}", x.id, x.name));
            let z = st.point_at(t).z;
            let what = match st.kind {
                hc_sim::StepKind::Rapid => "Rapid".to_string(),
                hc_sim::StepKind::Feed => format!("F {}/min", fmt_len(st.feed, u)),
                hc_sim::StepKind::Dwell => format!("Spindle start S{}", st.rpm),
            };
            ui.label(format!(
                "{op} · {tool} · Z {} · {what} · {} / {}",
                fmt_len(z, u),
                hc_sim::fmt_duration(t),
                hc_sim::fmt_duration(total)
            ));
        });
        ui.horizontal(|ui| {
            for (k, b) in sim.blocks.iter().enumerate() {
                ui.label(format!(
                    "File {} · T{} · {}",
                    k + 1,
                    b.tool,
                    hc_sim::fmt_duration(b.t1 - b.t0)
                ));
                ui.separator();
            }
            ui.label(format!("Total {}", hc_sim::fmt_duration(total)))
                .on_hover_text(format!(
                    "Estimate: rapids at {} mm/min, no acceleration, tool changes not counted",
                    hc_sim::DEFAULT_RAPID
                ));
            warn_skipped(ui);
        });
    });
}

/// "Error" / "Warning" in its colour, then " · <op> · <message>" in the text colour.
pub fn issue_job(ui: &Ui, word: &str, rest: &str) -> egui::text::LayoutJob {
    let v = ui.visuals();
    let color = if word == "Error" {
        v.error_fg_color
    } else {
        v.warn_fg_color
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(word, 0.0, egui::TextFormat::simple(font.clone(), color));
    job.append(
        &format!(" · {rest}"),
        0.0,
        egui::TextFormat::simple(font, v.text_color()),
    );
    job
}

/// Bottom panel above the status bar; a click selects the op (and the item).
pub fn issues(app: &mut App, ui: &mut Ui) {
    if app.state.issues.is_empty() {
        return;
    }
    let mut clicked = None;
    egui::Panel::bottom("issues")
        .resizable(true)
        .default_size(120.0)
        .show(ui, |ui| {
            ui.heading(format!("Issues ({})", app.state.issues.len()));
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (k, i) in app.state.issues.iter().enumerate() {
                        let (word, rest) = app.state.issue_parts(i);
                        if ui
                            .selectable_label(false, issue_job(ui, word, &rest))
                            .clicked()
                        {
                            clicked = Some(k);
                        }
                    }
                });
        });
    if let Some(k) = clicked {
        let i = app.state.issues[k].clone();
        app.state.select_issue(&i);
    }
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
            let r = ui.checkbox(&mut en, "");
            let label = format!("Enable {}", op.name);
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, en, &label));
            if r.changed() {
                act = Some(Act::Enable(op.id, en));
            }
            // kind only when the name doesn't already say it (default names do: "Contour 1")
            let kind = kind_label(&op.kind);
            let text = if op.name.starts_with(kind) {
                format!("{} · T{}", op.name, op.tool)
            } else {
                format!("{} · {} · T{}", op.name, kind, op.tool)
            };
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
        if let Some((text, errors)) = app.state.op_issue_summary(op.id) {
            let v = ui.visuals();
            let color = if errors {
                v.error_fg_color
            } else {
                v.warn_fg_color
            };
            ui.colored_label(color, text);
        }
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
        let l = ui.label("Tool");
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .show_ui(ui, |ui| {
                for t in tools {
                    ui.selectable_value(tool, t.id, format!("T{} {}", t.id, t.name));
                }
            })
            .response
            .labelled_by(l.id);
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
        let l = ui.label("Name");
        ui.text_edit_singleline(&mut op.name).labelled_by(l.id);
        ui.end_row();
        let l = ui.label("Enabled");
        ui.checkbox(&mut op.enabled, "").labelled_by(l.id);
        ui.end_row();
        ui.label("Kind");
        ui.label(kind_label(&op.kind));
        ui.end_row();
        match &mut op.kind {
            OpKind::Engrave => {}
            OpKind::Contour { side, direction } => {
                let l = ui.label("Side");
                egui::ComboBox::from_id_salt("side")
                    .selected_text(format!("{side:?}"))
                    .show_ui(ui, |ui| {
                        for s in [Side::Outside, Side::Inside, Side::On] {
                            ui.selectable_value(side, s, format!("{s:?}"));
                        }
                    })
                    .response
                    .labelled_by(l.id);
                ui.end_row();
                let l = ui.label("Direction");
                egui::ComboBox::from_id_salt("dir")
                    .selected_text(format!("{direction:?}"))
                    .show_ui(ui, |ui| {
                        for d in [Direction::Climb, Direction::Conventional] {
                            ui.selectable_value(direction, d, format!("{d:?}"));
                        }
                    })
                    .response
                    .labelled_by(l.id);
                ui.end_row();
            }
            OpKind::Pocket { stepover_pct } => {
                let l = ui.label("Stepover");
                ui.add(
                    egui::DragValue::new(stepover_pct)
                        .range(1.0..=100.0)
                        .suffix(" %"),
                )
                .labelled_by(l.id);
                ui.end_row();
            }
            OpKind::Drill { peck } => {
                let l = ui.label("Peck");
                ui.horizontal(|ui| {
                    let mut on = peck.is_some();
                    if ui.checkbox(&mut on, "").labelled_by(l.id).changed() {
                        *peck = on.then_some(op.depth_per_pass);
                    }
                    if let Some(p) = peck {
                        length_field(ui, p, u, Quantity::Length).labelled_by(l.id);
                    }
                });
                ui.end_row();
            }
        }
        let l = ui.label("Depth");
        length_field(ui, &mut op.depth, u, Quantity::Length).labelled_by(l.id);
        ui.end_row();
        let l = ui.label("Depth/pass");
        length_field(ui, &mut op.depth_per_pass, u, Quantity::Length).labelled_by(l.id);
        ui.end_row();
        let l = ui.label("Feed");
        length_field(ui, &mut op.feed, u, Quantity::Feed).labelled_by(l.id);
        ui.end_row();
        let l = ui.label("Plunge feed");
        length_field(ui, &mut op.plunge_feed, u, Quantity::Feed).labelled_by(l.id);
        ui.end_row();
        let l = ui.label("RPM");
        ui.add(egui::DragValue::new(&mut op.rpm).range(0..=100_000))
            .labelled_by(l.id);
        ui.end_row();
    });
    let mut reset = false;
    if matches!(op.kind, OpKind::Contour { .. }) {
        reset = tabs_group(ui, &mut op.tabs, u);
    }
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
    if reset {
        app.state.reset_tabs(id);
    }
}

/// Tab settings of a Contour op; true when "Reset to auto" was clicked.
fn tabs_group(ui: &mut Ui, tabs: &mut hc_project::Tabs, u: Units) -> bool {
    let mut reset = false;
    ui.group(|ui| {
        ui.strong("Tabs");
        ui.checkbox(&mut tabs.enabled, "Tabs enabled");
        ui.add_enabled_ui(tabs.enabled, |ui| {
            ui.horizontal(|ui| {
                let is_count = matches!(tabs.placement, TabPlacement::Count { .. });
                let r = ui.radio(is_count, "Count");
                if r.clicked() && !is_count {
                    tabs.placement = TabPlacement::Count { count: 4 };
                }
                if let TabPlacement::Count { count } = &mut tabs.placement {
                    ui.add(egui::DragValue::new(count).range(0..=100))
                        .labelled_by(r.id);
                }
            });
            ui.horizontal(|ui| {
                let is_sp = matches!(tabs.placement, TabPlacement::Spacing { .. });
                let r = ui.radio(is_sp, "Spacing");
                if r.clicked() && !is_sp {
                    tabs.placement = TabPlacement::Spacing { distance: 100.0 };
                }
                if let TabPlacement::Spacing { distance } = &mut tabs.placement {
                    length_field(ui, distance, u, Quantity::Length).labelled_by(r.id);
                }
            });
            egui::Grid::new("tab_props").num_columns(2).show(ui, |ui| {
                let l = ui.label("Width");
                length_field(ui, &mut tabs.width, u, Quantity::Length).labelled_by(l.id);
                ui.end_row();
                let l = ui.label("Height");
                length_field(ui, &mut tabs.height, u, Quantity::Length).labelled_by(l.id);
                ui.end_row();
            });
            reset = ui
                .add_enabled(!tabs.manual.is_empty(), egui::Button::new("Reset to auto"))
                .clicked();
            ui.weak("Double-click the contour to add, Del removes");
        });
    });
    reset
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
                if let Some(circle) = c.as_circle() {
                    ui.label("Ø");
                    ui.label(len(2.0 * circle.radius));
                    ui.end_row();
                }
            }
        }
    });
}
