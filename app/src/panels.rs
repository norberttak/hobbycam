//! Menu, side panels, message panel and status bar. All edits go through `AppState`.

use crate::state::{
    fmt_angle, kind_label, measure_angle, measure_summary, CanvasTool, MoveDir, NewKind, Severity,
    Shape,
};
use crate::theme;
use crate::{aci, icons, App, Pending};
use eframe::egui::{self, KeyboardShortcut, Modifiers, Ui};
use hc_geom::{Point, Transform};
use hc_project::{
    fmt_len, from_display, parse_length, to_display, Direction, Geom, OpKind, Side, TabPlacement,
    Units, WorkOrigin,
};

#[derive(Clone, Copy)]
pub enum Quantity {
    Length,
    Feed,
}

/// Length/feed `DragValue`: shows display units, accepts `1/8`, `3 mm`, …, writes mm back.
pub fn length_field(ui: &mut Ui, mm: &mut f64, units: Units, q: Quantity) -> egui::Response {
    length_drag(ui, mm, units, q, true)
}

/// Property field: like a length field but typed text applies on Enter / focus loss only (typing "0.5" never applies "0").
fn prop_field(ui: &mut Ui, mm: &mut f64, units: Units) -> egui::Response {
    length_drag(ui, mm, units, Quantity::Length, false)
}

fn length_drag(ui: &mut Ui, mm: &mut f64, units: Units, q: Quantity, live: bool) -> egui::Response {
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
            .update_while_editing(live)
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
const SC_SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Comma);
const SC_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Z);
const SC_REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), egui::Key::Z);
const SC_REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, egui::Key::Y);

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
    if ctx.input_mut(|i| i.consume_shortcut(&SC_SETTINGS)) {
        app.settings_open = true;
    }
    // before the canvas reads Esc: closing the window must not also cancel the tool
    if app.settings_open
        && !app.kb_focus
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::Escape))
    {
        app.settings_open = false;
    }
    // a focused text field keeps its own undo
    if !ctx.egui_wants_keyboard_input() {
        // Shift+Z first, as for Save as
        if ctx.input_mut(|i| i.consume_shortcut(&SC_REDO) || i.consume_shortcut(&SC_REDO_Y)) {
            app.state.redo();
        } else if ctx.input_mut(|i| i.consume_shortcut(&SC_UNDO)) {
            app.state.undo();
        }
    }
}

/// Menu entry that can be disabled, with its shortcut text.
fn item_enabled(ui: &mut Ui, enabled: bool, text: &str, sc: &KeyboardShortcut) -> bool {
    let b = egui::Button::new(text).shortcut_text(ui.ctx().format_shortcut(sc));
    let r = ui.add_enabled(enabled, b);
    // accessible name without the shortcut text
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, text));
    r.clicked()
}

/// Draw menu entry: checked when `tool` is active.
fn tool_item(app: &mut App, ui: &mut Ui, text: &str, key: &str, tool: CanvasTool) {
    let on = app.state.tool == tool;
    let r = ui.add(egui::Button::new(text).selected(on).shortcut_text(key));
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, text));
    if r.clicked() {
        app.state.set_tool(tool);
    }
}

fn item(ui: &mut Ui, text: &str, sc: Option<&KeyboardShortcut>) -> bool {
    let mut b = egui::Button::new(text);
    if let Some(sc) = sc {
        b = b.shortcut_text(ui.ctx().format_shortcut(sc));
    }
    let r = ui.add(b);
    // accessible name without the shortcut text
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    r.clicked()
}

/// The published user manual (docs/, deployed to GitHub Pages in M12).
pub const DOCS_URL: &str = "https://norberttak.github.io/hobbycam/";

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
                if item(ui, "Settings…", Some(&SC_SETTINGS)) {
                    app.settings_open = true;
                }
                if item(ui, "Tool library…", None) {
                    app.tool_dialog.open = true;
                }
                ui.separator();
                if item(ui, "Quit", None) {
                    app.request(&ctx, Pending::Quit);
                }
            });
            ui.menu_button("Edit", |ui| {
                if item_enabled(ui, app.state.can_undo(), "Undo", &SC_UNDO) {
                    app.state.undo();
                }
                if item_enabled(ui, app.state.can_redo(), "Redo", &SC_REDO) {
                    app.state.redo();
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
            ui.menu_button("Draw", |ui| {
                tool_item(app, ui, "Select", "Esc", CanvasTool::Select);
                tool_item(app, ui, "Line", "L", CanvasTool::Line);
                tool_item(app, ui, "Rectangle", "R", CanvasTool::Rect);
                tool_item(app, ui, "Circle", "C", CanvasTool::Circle);
                tool_item(app, ui, "Text", "T", CanvasTool::Text);
                tool_item(app, ui, "Round corners", "", CanvasTool::Fillet);
                tool_item(app, ui, "Measure", "", CanvasTool::Measure);
                ui.separator();
                tool_item(app, ui, "Set origin", "O", CanvasTool::SetOrigin);
            });
            ui.menu_button("Help", |ui| {
                if ui.button("User manual").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(DOCS_URL));
                }
            });
        });
    });
}

/// Units, Machine and Stock in one window; edits apply live (undo and dirty as for any edit).
pub fn settings_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.settings_open;
    if !open {
        return;
    }
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(egui::pos2(400.0, 60.0))
        .show(ctx, |ui| {
            ui.heading("Units");
            units_section(app, ui);
            ui.separator();
            ui.heading("Machine");
            machine_section(app, ui);
            ui.separator();
            ui.heading("Stock");
            stock_section(app, ui);
        });
    // Pick on drawing may have closed it during show
    app.settings_open &= open;
}

fn units_section(app: &mut App, ui: &mut Ui) {
    let u = app.state.project.units;
    if ui.radio(u == Units::Mm, "mm").clicked() {
        app.state.set_units(Units::Mm);
    }
    if ui.radio(u == Units::Inch, "inch").clicked() {
        app.state.set_units(Units::Inch);
    }
}

fn machine_section(app: &mut App, ui: &mut Ui) {
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
}

/// One sidebar icon button: 40 px, the glyph at 24 px, tooltip "<name> (<shortcut>)", accessible name = `name`.
fn side_button(
    ui: &mut Ui,
    glyph: &str,
    name: &str,
    shortcut: Option<String>,
    enabled: bool,
    selected: Option<bool>,
    tint: theme::Tint,
) -> egui::Response {
    let r = ui.add_enabled(enabled, side_widget(glyph, tint, selected.unwrap_or(false)));
    side_info(&r, name, shortcut, enabled, selected)
}

/// Painted tile (M18): group colour, white glyph; selected = gap + dark ring + arrow on the left (outside the 40 px rect).
fn side_widget(glyph: &str, tint: theme::Tint, selected: bool) -> impl egui::Widget + '_ {
    use egui::{pos2, vec2, Align2, FontId, Sense, Shape, Stroke, StrokeKind};
    move |ui: &mut Ui| {
        let (rect, r) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::click());
        if ui.is_rect_visible(rect) {
            let st = theme::tile_style(
                tint,
                ui.visuals().dark_mode,
                ui.is_enabled(),
                selected,
                r.hovered(),
                r.is_pointer_button_down_on(),
            );
            let mut p = ui.painter().clone();
            p.set_opacity(1.0); // disabled tiles are already grey; do not fade twice
            p.rect_filled(rect, 8, st.fill);
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(22.0),
                st.icon,
            );
            if let Some((gap, ring)) = st.ring {
                p.rect_stroke(rect, 8, Stroke::new(2.5, gap), StrokeKind::Outside);
                p.rect_stroke(
                    rect.expand(2.5),
                    10,
                    Stroke::new(2.5, ring),
                    StrokeKind::Outside,
                );
                let (l, cy) = (rect.left(), rect.center().y);
                p.add(Shape::convex_polygon(
                    vec![
                        pos2(l - 6.0, cy),
                        pos2(l - 11.0, cy - 5.0),
                        pos2(l - 11.0, cy + 5.0),
                    ],
                    ring,
                    Stroke::NONE,
                ));
            }
            if r.has_focus() {
                p.rect_stroke(
                    rect,
                    8,
                    Stroke::new(2.0, ui.visuals().hyperlink_color),
                    StrokeKind::Inside,
                );
            }
        }
        r
    }
}

fn side_info(
    r: &egui::Response,
    name: &str,
    shortcut: Option<String>,
    enabled: bool,
    selected: Option<bool>,
) -> egui::Response {
    r.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(egui::Role::Button, enabled, on, name),
        None => egui::WidgetInfo::labeled(egui::Role::Button, enabled, name),
    });
    let tip = match shortcut {
        Some(sc) => format!("{name} ({sc})"),
        None => name.to_string(),
    };
    r.clone().on_hover_text(tip).on_disabled_hover_text(name)
}

pub(crate) const TILE: f32 = 36.0;
/// Gap between the two button columns (room for the selected ring and the arrow of the right column).
const SIDEBAR_COL_GAP: f32 = 18.0;
/// Gap between rows (room for the 5 px ring).
const SIDEBAR_ROW_GAP: f32 = 6.0;
/// Extra left padding so the left column's arrow is not clipped.
const SIDEBAR_PAD_LEFT: f32 = 4.0;
/// Two tiles, the column gap, the extra left padding and the panel margins (8 px each side).
pub(crate) const SIDEBAR_W: f32 = 2.0 * TILE + SIDEBAR_COL_GAP + SIDEBAR_PAD_LEFT + 16.0;

/// What a sidebar button does; applied after the bar is drawn (one borrow of `app` at a time).
#[derive(Clone, Copy)]
enum SideAct {
    Tool(CanvasTool),
    Fit,
    Undo,
    Redo,
    Delete,
    Rotate,
    Flip(bool),
    New,
    Open,
    Save,
    Import,
    Export,
    Toolpaths,
    Sim,
    Tools,
    Settings,
}

impl SideEntry {
    /// Tile colour group.
    pub(crate) fn tint(&self) -> theme::Tint {
        use theme::Tint;
        match self.act {
            SideAct::Tool(CanvasTool::SetOrigin | CanvasTool::Measure) | SideAct::Fit => Tint::View,
            SideAct::Tool(_) => Tint::Draw,
            SideAct::Undo
            | SideAct::Redo
            | SideAct::Delete
            | SideAct::Rotate
            | SideAct::Flip(_) => Tint::Edit,
            SideAct::New | SideAct::Open | SideAct::Save | SideAct::Import | SideAct::Export => {
                Tint::File
            }
            SideAct::Toolpaths | SideAct::Sim => Tint::Ops,
            SideAct::Tools | SideAct::Settings => Tint::Set,
        }
    }
}

pub(crate) struct SideEntry {
    glyph: &'static str,
    pub(crate) name: &'static str,
    pub(crate) shortcut: Option<String>,
    enabled: bool,
    selected: Option<bool>,
    act: SideAct,
}

fn entry(
    glyph: &'static str,
    name: &'static str,
    shortcut: Option<String>,
    act: SideAct,
) -> SideEntry {
    SideEntry {
        glyph,
        name,
        shortcut,
        enabled: true,
        selected: None,
        act,
    }
}

/// Left icon bar, two columns: every menu action except Save as…, Quit and Help, plus Delete / Rotate / Flip of the selection.
pub fn sidebar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    egui::Panel::left("sidebar")
        .resizable(false)
        .exact_size(SIDEBAR_W)
        .show(ui, |ui| {
            // fallback only: at 1280 x 800 every button is visible (kittest m10a2_sidebar_fits)
            egui::ScrollArea::vertical().show(ui, |ui| sidebar_buttons(app, ui, &ctx));
        });
}

pub(crate) fn sidebar_groups(app: &App, ctx: &egui::Context) -> Vec<Vec<SideEntry>> {
    let sc = |s: &KeyboardShortcut| Some(ctx.format_shortcut(s));
    // empty = no shortcut (no empty "()" in the tooltip)
    let key = |k: &str| (!k.is_empty()).then(|| k.to_string());
    let s = &app.state;
    let tool = |glyph, name, k, t: CanvasTool| SideEntry {
        selected: Some(s.tool == t),
        ..entry(glyph, name, key(k), SideAct::Tool(t))
    };
    let when = |on: bool, e: SideEntry| SideEntry { enabled: on, ..e };
    let toggle = |on: bool, e: SideEntry| SideEntry {
        selected: Some(on),
        ..e
    };
    let sel = !s.selection.is_empty();
    vec![
        vec![
            tool(icons::SELECT, "Select", "Esc", CanvasTool::Select),
            tool(icons::LINE, "Line", "L", CanvasTool::Line),
            tool(icons::RECT, "Rectangle", "R", CanvasTool::Rect),
            tool(icons::CIRCLE, "Circle", "C", CanvasTool::Circle),
            tool(icons::TEXT, "Text", "T", CanvasTool::Text),
            tool(icons::FILLET, "Round corners", "", CanvasTool::Fillet),
            tool(icons::ORIGIN, "Set origin", "O", CanvasTool::SetOrigin),
            tool(icons::MEASURE, "Measure", "", CanvasTool::Measure),
        ],
        vec![
            when(
                s.can_undo(),
                entry(icons::UNDO, "Undo", sc(&SC_UNDO), SideAct::Undo),
            ),
            when(
                s.can_redo(),
                entry(icons::REDO, "Redo", sc(&SC_REDO), SideAct::Redo),
            ),
            when(
                sel,
                entry(
                    icons::DELETE,
                    "Delete selection",
                    key("Del"),
                    SideAct::Delete,
                ),
            ),
            when(
                sel,
                entry(icons::ROTATE, "Rotate selection", None, SideAct::Rotate),
            ),
            when(
                sel,
                entry(
                    icons::FLIP_H,
                    "Flip selection horizontally",
                    None,
                    SideAct::Flip(true),
                ),
            ),
            when(
                sel,
                entry(
                    icons::FLIP_V,
                    "Flip selection vertically",
                    None,
                    SideAct::Flip(false),
                ),
            ),
        ],
        vec![
            entry(icons::NEW, "New", sc(&SC_NEW), SideAct::New),
            entry(icons::OPEN, "Open project…", sc(&SC_OPEN), SideAct::Open),
            entry(icons::SAVE, "Save", sc(&SC_SAVE), SideAct::Save),
            entry(
                icons::IMPORT,
                "Import DXF…",
                sc(&SC_IMPORT),
                SideAct::Import,
            ),
            entry(
                icons::EXPORT,
                "Export G-code…",
                sc(&SC_EXPORT),
                SideAct::Export,
            ),
            entry(icons::FIT, "Zoom to fit", key("F"), SideAct::Fit),
        ],
        vec![
            toggle(
                s.show_toolpaths,
                entry(icons::TOOLPATHS, "Show toolpaths", None, SideAct::Toolpaths),
            ),
            toggle(
                s.sim_view.open,
                entry(icons::SIM, "Simulation", None, SideAct::Sim),
            ),
        ],
        vec![
            entry(icons::TOOLS, "Tool library…", None, SideAct::Tools),
            entry(
                icons::SETTINGS,
                "Settings…",
                sc(&SC_SETTINGS),
                SideAct::Settings,
            ),
        ],
    ]
}

fn sidebar_buttons(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    ui.spacing_mut().item_spacing = egui::vec2(SIDEBAR_COL_GAP, SIDEBAR_ROW_GAP);
    let groups = sidebar_groups(app, ctx);
    let mut clicked = None;
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: SIDEBAR_PAD_LEFT as i8,
            right: 0,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            for (g, group) in groups.into_iter().enumerate() {
                if g > 0 {
                    ui.separator();
                }
                // two per row; an odd group leaves its last cell empty
                for row in group.chunks(2) {
                    ui.horizontal(|ui| {
                        for e in row {
                            if side_button(
                                ui,
                                e.glyph,
                                e.name,
                                e.shortcut.clone(),
                                e.enabled,
                                e.selected,
                                e.tint(),
                            )
                            .clicked()
                            {
                                clicked = Some(e.act);
                            }
                        }
                    });
                }
            }
        });
    let s = &mut app.state;
    match clicked {
        None => {}
        Some(SideAct::Tool(t)) => s.set_tool(t),
        Some(SideAct::Fit) => s.fit_pending = true,
        Some(SideAct::Undo) => {
            s.undo();
        }
        Some(SideAct::Redo) => {
            s.redo();
        }
        Some(SideAct::Delete) => s.delete_selection(),
        Some(SideAct::Rotate) => s.rotate_selection(),
        Some(SideAct::Flip(h)) => s.flip_selection(h),
        Some(SideAct::Toolpaths) => s.show_toolpaths = !s.show_toolpaths,
        Some(SideAct::Sim) => s.sim_view.open = !s.sim_view.open,
        Some(SideAct::New) => app.request(ctx, Pending::New),
        Some(SideAct::Open) => app.request(ctx, Pending::Open),
        Some(SideAct::Save) => {
            app.save();
        }
        Some(SideAct::Import) => app.import_dxf(),
        Some(SideAct::Export) => app.export_gcode(),
        Some(SideAct::Tools) => app.tool_dialog.open = true,
        Some(SideAct::Settings) => app.settings_open = true,
    }
}

fn stock_section(app: &mut App, ui: &mut Ui) {
    let u = app.state.project.units;
    let mut st = app.state.project.stock.clone();
    egui::Grid::new("stock").show(ui, |ui| {
        for (name, v) in [
            ("X", &mut st.min_x),
            ("Y", &mut st.min_y),
            ("Width", &mut st.width),
            ("Height", &mut st.height),
            ("Thickness", &mut st.thickness),
        ] {
            let l = ui.label(name);
            length_field(ui, v, u, Quantity::Length).labelled_by(l.id);
            ui.end_row();
        }
    });
    ui.separator();
    ui.label("Origin");
    for (name, o) in [
        ("Drawing origin", WorkOrigin::DrawingOrigin),
        ("Stock bottom left", WorkOrigin::StockBottomLeft),
        ("Stock center", WorkOrigin::StockCenter),
    ] {
        if ui.radio(st.origin == o, name).clicked() {
            st.origin = o;
        }
    }
    let picked = matches!(st.origin, WorkOrigin::Point(_));
    if ui.radio(picked, "Picked point").clicked() && !picked {
        // start at the current zero: no jump
        st.origin = WorkOrigin::Point(hc_post::work_origin(&app.state.project));
    }
    if let WorkOrigin::Point(p) = &mut st.origin {
        egui::Grid::new("stock_origin").show(ui, |ui| {
            let l = ui.label("Origin X");
            length_field(ui, &mut p.x, u, Quantity::Length).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Origin Y");
            length_field(ui, &mut p.y, u, Quantity::Length).labelled_by(l.id);
            ui.end_row();
        });
    }
    app.state.set_stock(st);
    if ui.button("Pick on drawing").clicked() {
        app.state.set_tool(CanvasTool::SetOrigin);
        app.settings_open = false;
    }
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
            let o = hc_post::work_origin(&s.project);
            if let Some(p) = s.cursor.filter(|_| o.length() > 0.0) {
                ui.monospace(format!(
                    "· Work X {:.dec$} Y {:.dec$}",
                    to_display(p.x - o.x, u),
                    to_display(p.y - o.y, u)
                ));
            }
            ui.separator();
            ui.label(format!("Zoom {:.0}%", s.view.zoom * 100.0));
            ui.separator();
            ui.label(format!("{} selected", s.selection.len()));
            if let Some(m) = &s.moving {
                ui.separator();
                ui.label(format!(
                    "Move X {} Y {} · Shift: straight · Esc cancels",
                    fmt_len(m.delta.x, u),
                    fmt_len(m.delta.y, u)
                ));
            }
            if let Some((a, b)) = s.measure_live() {
                ui.separator();
                ui.label(measure_summary(a, b, u));
            }
            if let Some(h) = s.tool.hint(s.pending.is_some()) {
                ui.separator();
                let cap = if s.snap_cache.capped {
                    " · Intersection snaps off: drawing too large"
                } else {
                    ""
                };
                ui.label(format!("{h} · Esc cancels{cap}"));
            }
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
                    ui.weak("No tools: add one in File > Tool library…");
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
    let s = &app.state;
    let drawing = !matches!(s.tool, CanvasTool::Select | CanvasTool::SetOrigin);
    let op = s
        .selected_op
        .is_some_and(|id| s.project.operations.iter().any(|o| o.id == id));
    if !drawing && !op && s.selection.is_empty() {
        return; // nothing selected: no Properties pane
    }
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
                    _ if app.state.tool == CanvasTool::Measure => measure_form(app, ui),
                    _ if !matches!(app.state.tool, CanvasTool::Select | CanvasTool::SetOrigin) => {
                        draw_form(app, ui)
                    }
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

/// Read-only Measure read-out in Properties (writes nothing).
fn measure_form(app: &mut App, ui: &mut Ui) {
    let s = &app.state;
    let u = s.project.units;
    ui.group(|ui| {
        ui.strong("Measure");
        if let Some((a, b)) = s.measure_live() {
            egui::Grid::new("measure_form")
                .num_columns(2)
                .show(ui, |ui| {
                    let d = b - a;
                    let angle = fmt_angle(measure_angle(a, b));
                    let rows = [
                        ("A X", fmt_len(a.x, u)),
                        ("A Y", fmt_len(a.y, u)),
                        ("B X", fmt_len(b.x, u)),
                        ("B Y", fmt_len(b.y, u)),
                        ("Distance", fmt_len(d.length(), u)),
                        ("ΔX", fmt_len(d.x, u)),
                        ("ΔY", fmt_len(d.y, u)),
                        ("Angle", angle),
                    ];
                    for (k, v) in rows {
                        ui.label(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
        }
        ui.weak(s.tool.hint(s.pending.is_some()).unwrap_or_default());
        ui.weak("Drawing coordinates; not saved");
    });
}

/// Numeric input of the active drawing tool; canvas clicks write into the same fields.
fn draw_form(app: &mut App, ui: &mut Ui) {
    let s = &mut app.state;
    let u = s.project.units;
    let tool = s.tool;
    let has_sel = !s.selection.is_empty();
    let f = &mut s.draw;
    let title = match tool {
        CanvasTool::Line => "Draw line",
        CanvasTool::Rect => "Draw rectangle",
        CanvasTool::Text => "Draw text",
        CanvasTool::Fillet => "Round corners",
        _ => "Draw circle",
    };
    let (mut add, mut round_all) = (false, false);
    ui.group(|ui| {
        ui.strong(title);
        egui::Grid::new("draw_form").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut Ui, name: &str, v: &mut f64| {
                let l = ui.label(name);
                length_field(ui, v, u, Quantity::Length).labelled_by(l.id);
                ui.end_row();
            };
            match tool {
                CanvasTool::Line => {
                    row(ui, "X1", &mut f.a.x);
                    row(ui, "Y1", &mut f.a.y);
                    row(ui, "X2", &mut f.b.x);
                    row(ui, "Y2", &mut f.b.y);
                }
                CanvasTool::Rect => {
                    row(ui, "X", &mut f.a.x);
                    row(ui, "Y", &mut f.a.y);
                    row(ui, "Width", &mut f.w);
                    row(ui, "Height", &mut f.h);
                    row(ui, "Corner radius", &mut f.r);
                }
                CanvasTool::Text => {
                    let l = ui.label("Text");
                    // the typed text is geometry, not UI: any character is allowed, missing ones are reported on Add
                    ui.text_edit_singleline(&mut f.text).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label("Font");
                    let name = |font| match font {
                        hc_text::Font::Hershey => "Single-line (Hershey)",
                        hc_text::Font::Outline => "Outline (Ubuntu Light)",
                    };
                    egui::ComboBox::from_id_salt("text_font")
                        .selected_text(name(f.font))
                        .show_ui(ui, |ui| {
                            for font in [hc_text::Font::Hershey, hc_text::Font::Outline] {
                                ui.selectable_value(&mut f.font, font, name(font));
                            }
                        })
                        .response
                        .labelled_by(l.id);
                    ui.end_row();
                    row(ui, "X", &mut f.a.x);
                    row(ui, "Y", &mut f.a.y);
                    row(ui, "Height", &mut f.height);
                }
                CanvasTool::Fillet => row(ui, "Radius", &mut f.fillet_r),
                _ => {
                    row(ui, "Center X", &mut f.a.x);
                    row(ui, "Center Y", &mut f.a.y);
                    row(ui, "Diameter", &mut f.d);
                }
            }
        });
        if tool == CanvasTool::Fillet {
            round_all = ui
                .add_enabled(has_sel, egui::Button::new("Round all corners of selection"))
                .clicked();
            ui.weak("Or click a corner on the canvas");
        } else {
            add = ui.button("Add").clicked();
        }
    });
    if round_all {
        s.fillet_selection();
    }
    if let Some(shape) = add.then(|| s.draw.shape(tool)).flatten() {
        if let Err(e) = s.add_shape(shape) {
            s.msg(Severity::Warning, e);
        }
    }
}

/// Move / rotate / scale / flip / delete of the selection, about its bbox centre.
fn transform_group(app: &mut App, ui: &mut Ui) {
    let s = &mut app.state;
    let u = s.project.units;
    let c = s.selection_center();
    let x = &mut s.xform;
    let mut t = None;
    let mut delete = false;
    let mut rotate = false;
    let mut flip = None;
    ui.group(|ui| {
        ui.strong("Transform");
        egui::Grid::new("xform").num_columns(2).show(ui, |ui| {
            let l = ui.label("Move X");
            length_field(ui, &mut x.dx, u, Quantity::Length).labelled_by(l.id);
            ui.end_row();
            let l = ui.label("Move Y");
            length_field(ui, &mut x.dy, u, Quantity::Length).labelled_by(l.id);
            ui.end_row();
        });
        if ui.button("Move").clicked() {
            t = Some(Transform::translate(x.dx, x.dy));
        }
        ui.horizontal(|ui| {
            let l = ui.label("Angle");
            ui.add(egui::DragValue::new(&mut x.angle_deg).suffix("°"))
                .labelled_by(l.id);
            rotate = ui.button("Rotate").clicked();
        });
        ui.horizontal(|ui| {
            let l = ui.label("Factor");
            ui.add(egui::DragValue::new(&mut x.factor).speed(0.01))
                .labelled_by(l.id);
            let ok = x.factor.is_finite() && x.factor > 0.0;
            if ui.add_enabled(ok, egui::Button::new("Scale")).clicked() {
                t = Some(Transform::scale(x.factor, c));
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Flip horizontally").clicked() {
                flip = Some(true);
            }
            if ui.button("Flip vertically").clicked() {
                flip = Some(false);
            }
        });
        delete = ui.button("Delete").clicked();
        ui.weak("About the center of the selection");
    });
    if let Some(t) = t {
        s.transform_selection(&t);
    }
    if rotate {
        s.rotate_selection();
    }
    if let Some(h) = flip {
        s.flip_selection(h);
    }
    if delete {
        s.delete_selection();
    }
}

/// One editable property row: label + field. The response says whether it changed / is being dragged.
fn prop(ui: &mut Ui, label: &str, mm: &mut f64, u: Units) -> egui::Response {
    let l = ui.label(label);
    let r = prop_field(ui, mm, u).labelled_by(l.id);
    ui.end_row();
    r
}

fn prop_text(ui: &mut Ui, label: &str, text: impl Into<egui::WidgetText>) {
    ui.label(label);
    ui.label(text);
    ui.end_row();
}

/// What a changed property row asks for.
enum PropEdit {
    Shape(u64, crate::state::Shape),
    MoveTo(Point),
}

fn selection_props(app: &mut App, ui: &mut Ui) {
    let n = app.state.selection.len();
    ui.label(format!("{n} selected"));
    if n == 0 {
        return;
    }
    let s = &app.state;
    let u = s.project.units;
    let len = |mm: f64| fmt_len(mm, u);
    let mut edit: Option<(PropEdit, bool)> = None;
    egui::Grid::new("item_props").num_columns(2).show(ui, |ui| {
        let item = (n == 1)
            .then(|| s.item(*s.selection.first().unwrap()))
            .flatten();
        let id = item.map(|i| i.id);
        if let Some(it) = item {
            prop_text(ui, "Layer", &it.layer);
        }
        let b = s.selection_bbox();
        let mut min = b.min;
        let shape = id.and_then(|id| s.item_shape(id));
        let mut xy =
            |ui: &mut Ui, ex: &mut Option<(PropEdit, bool)>, x_label: &str, y_label: &str| {
                let rx = prop(ui, x_label, &mut min.x, u);
                let ry = prop(ui, y_label, &mut min.y, u);
                if rx.changed() || ry.changed() {
                    *ex = Some((PropEdit::MoveTo(min), rx.dragged() || ry.dragged()));
                }
            };
        match (id, shape) {
            (Some(id), Some(Shape::Line { a, b })) => {
                prop_text(ui, "Shape", "line");
                let (mut p, mut q) = (a, b);
                let mut l = a.distance(b);
                let rs = [
                    prop(ui, "X1", &mut p.x, u),
                    prop(ui, "Y1", &mut p.y, u),
                    prop(ui, "X2", &mut q.x, u),
                    prop(ui, "Y2", &mut q.y, u),
                ];
                let old = l;
                let rl = prop(ui, "Length", &mut l, u);
                if rl.changed() {
                    let k = if l > 0.0 { l / old } else { 0.0 };
                    q = a + (b - a) * k;
                }
                if rl.changed() || rs.iter().any(|r| r.changed()) {
                    let dragged = rl.dragged() || rs.iter().any(|r| r.dragged());
                    edit = Some((PropEdit::Shape(id, Shape::Line { a: p, b: q }), dragged));
                }
            }
            (Some(id), Some(Shape::Rect { min, w, h, r })) => {
                prop_text(ui, "Shape", "rectangle");
                let (mut m, mut w, mut h, mut r) = (min, w, h, r);
                let rs = [
                    prop(ui, "X", &mut m.x, u),
                    prop(ui, "Y", &mut m.y, u),
                    prop(ui, "Width", &mut w, u),
                    prop(ui, "Height", &mut h, u),
                    prop(ui, "Corner radius", &mut r, u),
                ];
                if rs.iter().any(|r| r.changed()) {
                    let dragged = rs.iter().any(|r| r.dragged());
                    edit = Some((
                        PropEdit::Shape(id, Shape::Rect { min: m, w, h, r }),
                        dragged,
                    ));
                }
            }
            (Some(id), Some(Shape::Circle { c, r })) => {
                prop_text(ui, "Shape", "circle");
                let (mut c, mut d) = (c, 2.0 * r);
                let rs = [
                    prop(ui, "Center X", &mut c.x, u),
                    prop(ui, "Center Y", &mut c.y, u),
                    prop(ui, "Diameter", &mut d, u),
                ];
                prop_text(ui, "Radius", len(r));
                if rs.iter().any(|r| r.changed()) {
                    let dragged = rs.iter().any(|r| r.dragged());
                    edit = Some((
                        PropEdit::Shape(id, Shape::Circle { c, r: d / 2.0 }),
                        dragged,
                    ));
                }
            }
            _ => {
                if let Some(Geom::Point(_)) = item.map(|i| &i.geom) {
                    prop_text(ui, "Shape", "point");
                }
                xy(ui, &mut edit, "X", "Y");
                if let Some(Geom::Contour(c)) = item.map(|i| &i.geom) {
                    prop_text(ui, "Shape", if c.closed { "closed" } else { "open" });
                    prop_text(ui, "Segments", c.segments.len().to_string());
                    prop_text(ui, "Length", len(c.length()));
                }
                if n > 1 || matches!(item.map(|i| &i.geom), Some(Geom::Contour(_))) {
                    prop_text(
                        ui,
                        "Size",
                        format!("{} × {}", len(b.width()), len(b.height())),
                    );
                }
            }
        }
    });
    match edit {
        Some((PropEdit::Shape(id, sh), dragged)) => {
            if let Err(e) = app.state.edit_shape(id, sh) {
                if !dragged {
                    app.state.msg(Severity::Warning, e);
                }
            }
        }
        Some((PropEdit::MoveTo(p), _)) => app.state.move_selection_to(p),
        None => {}
    }
    transform_group(app, ui);
}
