//! Application state and every project edit. No egui UI code here.

use crate::hit;
use crate::snap::{self, Snap, SnapCache};
use crate::view::View;
use hc_dxf::{ImportOptions, ImportWarning};
use hc_geom::{shapes, BBox, Contour, Point, Transform, EPS};
use hc_post::GcodeFile;
use hc_project::{
    fmt_len, Direction, Geom, Item, Machine, ManualTabs, OpKind, Operation, Project, Side, Stock,
    Tool, ToolLibrary, Units, WorkOrigin,
};
use hc_sim::{Sim, SimError};
use hc_toolpath::{Issue, Toolpath, ToolpathError};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDir {
    Up,
    Down,
}

/// The four operation kinds offered in the "New operation" form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    Engrave,
    Contour,
    Pocket,
    Drill,
}

impl NewKind {
    pub const ALL: [NewKind; 4] = [
        NewKind::Engrave,
        NewKind::Contour,
        NewKind::Pocket,
        NewKind::Drill,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NewKind::Engrave => "Engrave",
            NewKind::Contour => "Contour",
            NewKind::Pocket => "Pocket",
            NewKind::Drill => "Drill",
        }
    }

    fn op_kind(self) -> OpKind {
        match self {
            NewKind::Engrave => OpKind::Engrave,
            NewKind::Contour => OpKind::Contour {
                side: Side::Outside,
                direction: Direction::Climb,
            },
            // stepover is taken from the tool by add_operation
            NewKind::Pocket => OpKind::Pocket { stepover_pct: 0.0 },
            NewKind::Drill => OpKind::Drill { peck: None },
        }
    }
}

pub fn kind_label(k: &OpKind) -> &'static str {
    match k {
        OpKind::Engrave => "Engrave",
        OpKind::Contour { .. } => "Contour",
        OpKind::Pocket { .. } => "Pocket",
        OpKind::Drill { .. } => "Drill",
    }
}

/// Simulation playback; not saved.
#[derive(Debug, Clone, PartialEq)]
pub struct SimView {
    pub open: bool,
    pub playing: bool,
    /// Seconds into the job.
    pub t: f64,
    pub speed: f64,
}

impl Default for SimView {
    fn default() -> SimView {
        SimView {
            open: false,
            playing: false,
            t: 0.0,
            speed: 10.0,
        }
    }
}

impl SimView {
    /// Play/Pause; play at the end (`total`) restarts from 0.
    pub fn toggle(&mut self, total: f64) {
        if !self.playing && self.t >= total {
            self.t = 0.0;
        }
        self.playing = !self.playing;
    }
}

/// The active canvas tool; Select is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasTool {
    Select,
    SetOrigin,
    Line,
    Rect,
    Circle,
    Text,
}

impl CanvasTool {
    /// Status bar hint; `pending` = the first point is already set.
    pub fn hint(self, pending: bool) -> Option<&'static str> {
        Some(match (self, pending) {
            (CanvasTool::Select, _) => return None,
            (CanvasTool::SetOrigin, _) => "Set origin: click a snap point",
            (CanvasTool::Line, false) => "Line: click the start point",
            (CanvasTool::Line, true) => "Line: click the end point",
            (CanvasTool::Rect, false) => "Rectangle: click a corner",
            (CanvasTool::Rect, true) => "Rectangle: click the opposite corner",
            (CanvasTool::Circle, false) => "Circle: click the center",
            (CanvasTool::Circle, true) => "Circle: click a point on the circle",
            (CanvasTool::Text, _) => "Text: click the start of the baseline",
        })
    }
}

/// A shape to add; becomes a plain contour.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        a: Point,
        b: Point,
    },
    Rect {
        min: Point,
        w: f64,
        h: f64,
        r: f64,
    },
    Circle {
        c: Point,
        r: f64,
    },
    /// `at` = baseline start, `height` = cap height.
    Text {
        at: Point,
        text: String,
        font: hc_text::Font,
        height: f64,
    },
}

/// Persistent draw form values (mm). `a` = line start, rectangle bottom left, circle centre, text baseline start; `b` = line end.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawForm {
    pub a: Point,
    pub b: Point,
    pub w: f64,
    pub h: f64,
    pub r: f64,
    /// Circle diameter.
    pub d: f64,
    pub text: String,
    pub font: hc_text::Font,
    /// Text cap height.
    pub height: f64,
}

impl Default for DrawForm {
    fn default() -> DrawForm {
        DrawForm {
            a: Point::new(0.0, 0.0),
            b: Point::new(0.0, 0.0),
            w: 20.0,
            h: 20.0,
            r: 0.0,
            d: 10.0,
            text: "HobbyCAM".to_string(),
            font: hc_text::Font::Hershey,
            height: 10.0,
        }
    }
}

impl DrawForm {
    /// The shape of `tool` from the form values; None for Select / SetOrigin.
    pub fn shape(&self, tool: CanvasTool) -> Option<Shape> {
        match tool {
            CanvasTool::Line => Some(Shape::Line {
                a: self.a,
                b: self.b,
            }),
            CanvasTool::Rect => Some(Shape::Rect {
                min: self.a,
                w: self.w,
                h: self.h,
                r: self.r,
            }),
            CanvasTool::Circle => Some(Shape::Circle {
                c: self.a,
                r: self.d / 2.0,
            }),
            CanvasTool::Text => Some(Shape::Text {
                at: self.a,
                text: self.text.clone(),
                font: self.font,
                height: self.height,
            }),
            CanvasTool::Select | CanvasTool::SetOrigin => None,
        }
    }
}

/// Transform form values: move by (dx, dy) mm, rotate by `angle_deg` (CCW), scale by `factor`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XformForm {
    pub dx: f64,
    pub dy: f64,
    pub angle_deg: f64,
    pub factor: f64,
}

impl Default for XformForm {
    fn default() -> XformForm {
        XformForm {
            dx: 0.0,
            dy: 0.0,
            angle_deg: 90.0,
            factor: 2.0,
        }
    }
}

/// Undo history depth (whole-project snapshots).
/// shortcut: full clones, a few hundred MB for big DXF projects; lower the depth or share items via Arc if it bites.
pub const UNDO_DEPTH: usize = 100;

pub struct AppState {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub library: ToolLibrary,
    /// None when there is no config directory.
    pub library_path: Option<PathBuf>,
    /// Load error text; while set, the library cannot be saved.
    pub library_error: Option<String>,
    pub selection: BTreeSet<u64>,
    pub selected_op: Option<u64>,
    /// (item, index into that item's tab positions) of the selected op.
    pub selected_tab: Option<(u64, usize)>,
    pub hidden_layers: HashSet<String>,
    pub messages: Vec<Message>,
    pub view: View,
    /// Canvas fits the view on its next frame.
    pub fit_pending: bool,
    /// World position under the mouse, for the status bar.
    pub cursor: Option<Point>,
    /// Parallel to `project.items`.
    pub item_bbox: Vec<BBox>,
    /// One entry per enabled operation, in order (see `refresh_toolpaths`).
    pub toolpaths: Vec<(u64, Result<Toolpath, ToolpathError>)>,
    /// The project `toolpaths` were generated from.
    pub toolpaths_for: Option<Project>,
    /// `hc_toolpath::check` of `toolpaths`, refreshed with them.
    pub issues: Vec<Issue>,
    /// Not saved.
    pub show_toolpaths: bool,
    /// `hc_sim::simulate` of `toolpaths`, refreshed with them.
    pub sim: Result<Sim, SimError>,
    pub sim_view: SimView,
    pub tool: CanvasTool,
    pub draw: DrawForm,
    pub xform: XformForm,
    /// First click of a two-click drawing tool.
    pub pending: Option<Point>,
    /// (item index, point, kind), rebuilt in `items_changed`.
    pub snap_cache: SnapCache,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    /// The last committed state.
    pub undo_base: Project,
    /// The state on disk (or the fresh project).
    pub saved: Project,
}

impl AppState {
    /// Loads the tool library from `library_path` (tests pass a scratch path or None).
    pub fn new(library_path: Option<PathBuf>) -> AppState {
        let (library, library_error) = match &library_path {
            Some(p) => match ToolLibrary::load(p) {
                Ok(l) => (l, None),
                Err(e) => (
                    ToolLibrary::default(),
                    Some(format!("cannot load {}: {e}", p.display())),
                ),
            },
            None => (
                ToolLibrary::default(),
                Some("no user configuration directory found".to_string()),
            ),
        };
        let mut s = AppState {
            project: Project::new(),
            path: None,
            dirty: false,
            library,
            library_path,
            library_error,
            selection: BTreeSet::new(),
            selected_op: None,
            selected_tab: None,
            hidden_layers: HashSet::new(),
            messages: Vec::new(),
            view: View::default(),
            fit_pending: true,
            cursor: None,
            item_bbox: Vec::new(),
            toolpaths: Vec::new(),
            toolpaths_for: None,
            issues: Vec::new(),
            show_toolpaths: true,
            sim: Ok(Sim::default()),
            sim_view: SimView::default(),
            tool: CanvasTool::Select,
            draw: DrawForm::default(),
            xform: XformForm::default(),
            pending: None,
            snap_cache: SnapCache::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_base: Project::new(),
            saved: Project::new(),
        };
        if let Some(e) = s.library_error.clone() {
            s.msg(Severity::Error, format!("Tool library: {e}"));
        }
        s
    }

    pub fn msg(&mut self, severity: Severity, text: impl Into<String>) {
        self.messages.push(Message {
            severity,
            text: text.into(),
        });
    }

    /// Regenerates the toolpaths when the project differs from the one they were made for.
    /// shortcut: whole-project compare + full regeneration, may lag on huge fine pockets; upgrade to a per-op cache keyed by op + items + tool.
    pub fn refresh_toolpaths(&mut self) {
        if self.toolpaths_for.as_ref() != Some(&self.project) {
            self.toolpaths = hc_toolpath::generate(&self.project);
            self.issues = hc_toolpath::check(&self.project, &self.toolpaths);
            self.toolpaths_for = Some(self.project.clone());
            self.sim = hc_sim::simulate(&self.toolpaths, hc_sim::DEFAULT_RAPID);
            self.sim_view.t = self.sim_view.t.min(self.sim_total());
        }
    }

    /// Simulated job time in seconds (0 on a sim error).
    pub fn sim_total(&self) -> f64 {
        self.sim.as_ref().map_or(0.0, Sim::total)
    }

    /// Moves the playback by `dt` seconds of wall time; stops at the end.
    pub fn advance(&mut self, dt: f64) {
        let v = &mut self.sim_view;
        if !v.playing {
            return;
        }
        // a stalled frame does not jump the tool
        v.t += dt.min(0.1) * v.speed;
        let total = self.sim.as_ref().map_or(0.0, Sim::total);
        if v.t >= total {
            v.t = total;
            v.playing = false;
        }
    }

    pub fn toggle_play(&mut self) {
        let total = self.sim_total();
        self.sim_view.toggle(total);
    }

    pub fn toolpath_result(&self, op: u64) -> Option<&Result<Toolpath, ToolpathError>> {
        self.toolpaths
            .iter()
            .find(|(id, _)| *id == op)
            .map(|(_, r)| r)
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string());
        format!("HobbyCAM – {name}{}", if self.dirty { " *" } else { "" })
    }

    pub(crate) fn items_changed(&mut self) {
        self.item_bbox = self.project.items.iter().map(hit::item_bbox).collect();
        self.snap_cache = snap::build_cache(&self.project.items);
        let ids: HashSet<u64> = self.project.items.iter().map(|i| i.id).collect();
        self.selection.retain(|id| ids.contains(id));
    }

    /// Bbox of all items (empty when there are none).
    pub fn items_bbox(&self) -> BBox {
        self.item_bbox
            .iter()
            .fold(BBox::empty(), |acc, b| acc.union(b))
    }

    pub fn stock_bbox(&self) -> BBox {
        let s = &self.project.stock;
        BBox {
            min: Point::new(s.min_x, s.min_y),
            max: Point::new(s.min_x + s.width, s.min_y + s.height),
        }
    }

    pub fn is_visible(&self, item: &Item) -> bool {
        !self.hidden_layers.contains(&item.layer)
    }

    /// Replace the project with `p` (new/open).
    fn replace_project(&mut self, p: Project, path: Option<PathBuf>) {
        self.project = p;
        self.path = path;
        self.dirty = false;
        self.selection.clear();
        self.selected_op = None;
        self.selected_tab = None;
        self.hidden_layers.clear();
        self.items_changed();
        self.fit_pending = true;
        self.pending = None;
        self.undo.clear();
        self.redo.clear();
        self.undo_base = self.project.clone();
        self.saved = self.project.clone();
    }

    pub fn new_project(&mut self) {
        self.replace_project(Project::new(), None);
    }

    pub fn open(&mut self, path: &Path) -> bool {
        match Project::load(path) {
            // load problems show up in `issues` (M8)
            Ok((p, _)) => {
                self.replace_project(p, Some(path.to_path_buf()));
                true
            }
            Err(e) => {
                self.msg(
                    Severity::Error,
                    format!("Cannot open {}: {e}", path.display()),
                );
                false
            }
        }
    }

    /// Appends the DXF to the current project.
    pub fn import_dxf(&mut self, path: &Path) -> bool {
        let imp = match hc_dxf::import_dxf_file(path, &ImportOptions::default()) {
            Ok(i) => i,
            Err(e) => {
                self.msg(
                    Severity::Error,
                    format!("Cannot import {}: {e}", path.display()),
                );
                return false;
            }
        };
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ids = self.project.add_dxf(&imp, &file);
        self.items_changed();
        self.fit_stock_to_items();
        for w in &imp.warnings {
            self.msg(Severity::Warning, warning_text(w));
        }
        if !imp.ignored.is_empty() {
            let list: Vec<String> = imp
                .ignored
                .iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect();
            self.msg(
                Severity::Info,
                format!("{file}: Ignored: {}", list.join(", ")),
            );
        }
        if ids.is_empty() {
            self.msg(Severity::Warning, format!("{file}: no geometry imported"));
        }
        self.dirty = true;
        self.fit_pending = true;
        true
    }

    /// First import into an untouched project: stock = drawing bbox (thickness and origin kept).
    fn fit_stock_to_items(&mut self) {
        let b = self.items_bbox();
        // the origin choice does not count as touching the stock
        let fresh = Stock {
            origin: self.project.stock.origin,
            ..Project::new().stock
        };
        let untouched = self.project.stock == fresh && self.project.operations.is_empty();
        if untouched && !b.is_empty() && b.width() > 0.0 && b.height() > 0.0 {
            let s = &mut self.project.stock;
            s.min_x = b.min.x;
            s.min_y = b.min.y;
            s.width = b.width();
            s.height = b.height();
        }
    }

    /// `hobbycam <file>` argument.
    pub fn open_cli(&mut self, path: &Path) {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "dxf" => {
                self.new_project();
                self.import_dxf(path);
            }
            "hcam" => {
                self.open(path);
            }
            _ => self.msg(
                Severity::Error,
                format!(
                    "Cannot open {}: expected a .dxf or .hcam file",
                    path.display()
                ),
            ),
        }
    }

    /// Saves to the current path; false when there is none or the save failed (dirty stays set).
    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.save_as(&p),
            None => false,
        }
    }

    pub fn save_as(&mut self, path: &Path) -> bool {
        match self.project.save(path) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.saved = self.project.clone();
                true
            }
            Err(e) => {
                self.msg(
                    Severity::Error,
                    format!("Cannot save {}: {e}", path.display()),
                );
                false
            }
        }
    }

    pub fn set_machine(&mut self, m: Machine) {
        if self.project.machine != m {
            self.project.machine = m;
            self.dirty = true;
        }
    }

    pub fn set_stock(&mut self, s: Stock) {
        if self.project.stock != s {
            self.project.stock = s;
            self.dirty = true;
        }
    }

    pub fn set_origin(&mut self, o: WorkOrigin) {
        if self.project.stock.origin != o {
            self.project.stock.origin = o;
            self.dirty = true;
        }
    }

    /// Project file stem, or "untitled".
    pub fn stem(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string())
    }

    /// The G-code files of the project, headers named `name` (the export file name), or None with an Error message.
    pub fn gcode(&mut self, name: &str) -> Option<Vec<GcodeFile>> {
        match hc_post::export(&self.project, name) {
            Ok(files) => Some(files),
            Err(e) => {
                self.msg(Severity::Error, format!("Cannot export G-code: {e}"));
                None
            }
        }
    }

    /// Writes `<stem><suffix>.<ext>` next to `base` (its extension stripped), in order, stopping at the first failure.
    pub fn write_gcode(&mut self, base: &Path, files: &[GcodeFile]) -> bool {
        let ext = match hc_post::post_for(&self.project.machine.post) {
            Ok(p) => p.extension(),
            Err(e) => {
                self.msg(Severity::Error, format!("Cannot export G-code: {e}"));
                return false;
            }
        };
        let dir = base.parent().unwrap_or(Path::new(""));
        let stem = base
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut written: Vec<String> = Vec::new();
        for f in files {
            let name = format!("{stem}{}.{ext}", f.suffix);
            if let Err(e) = hc_project::atomic_write(&dir.join(&name), f.text.as_bytes()) {
                let done = if written.is_empty() {
                    "none".to_string()
                } else {
                    written.join(", ")
                };
                self.msg(
                    Severity::Error,
                    format!("Cannot write {name}: {e}. Written: {done}"),
                );
                return false;
            }
            written.push(name);
        }
        self.msg(
            Severity::Info,
            format!("Exported {} file(s): {}", written.len(), written.join(", ")),
        );
        // never deleted: an old "file 3 of 3" would machine the wrong part, so name them
        let single = format!("{stem}.{ext}");
        let (prefix, dot_ext) = (format!("{stem}_"), format!(".{ext}"));
        let mut stale: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .filter(|n| !written.contains(n))
                    .filter(|n| {
                        *n == single
                            || n.strip_prefix(&prefix)
                                .and_then(|r| r.strip_suffix(&dot_ext))
                                .is_some_and(|mid| mid.contains("_T"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !stale.is_empty() {
            stale.sort();
            self.msg(
                Severity::Warning,
                format!(
                    "Older files from a previous export are still in the folder: {}",
                    stale.join(", ")
                ),
            );
        }
        true
    }

    pub fn set_units(&mut self, u: Units) {
        if self.project.units != u {
            self.project.units = u;
            self.dirty = true;
        }
    }

    // ---------------------------------------------------------------- selection

    pub fn click_select(&mut self, p: Point, tol: f64, shift: bool) {
        let hit = hit::pick(&self.project.items, |i| self.is_visible(i), p, tol);
        if shift {
            // the op stays current, so the user can edit its geometry for "Use current selection"
            if let Some(id) = hit {
                if !self.selection.remove(&id) {
                    self.selection.insert(id);
                }
            }
            return;
        }
        // first click of a double-click on a tab contour: keep the op so the tab markers stay
        if hit.is_some_and(|id| self.tab_markers().iter().any(|(m, _, _)| *m == id)) {
            return;
        }
        self.selection = hit.into_iter().collect();
        self.selected_op = None;
        self.selected_tab = None;
    }

    pub fn box_select(&mut self, a: Point, b: Point, shift: bool, tol: f64) {
        let mut bb = BBox::empty();
        bb.include_point(a);
        bb.include_point(b);
        let crossing = b.x < a.x;
        let ids = hit::box_select(
            &self.project.items,
            |i| self.is_visible(i),
            &bb,
            crossing,
            tol,
        );
        if !shift {
            self.selection.clear();
            self.selected_op = None;
            self.selected_tab = None;
        }
        self.selection.extend(ids);
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn set_layer_visible(&mut self, layer: &str, visible: bool) {
        if visible {
            self.hidden_layers.remove(layer);
        } else {
            self.hidden_layers.insert(layer.to_string());
            let hidden: HashSet<u64> = self
                .project
                .items
                .iter()
                .filter(|i| i.layer == layer)
                .map(|i| i.id)
                .collect();
            self.selection.retain(|id| !hidden.contains(id));
        }
    }

    pub fn layer_item_count(&self, layer: &str) -> usize {
        self.project
            .items
            .iter()
            .filter(|i| i.layer == layer)
            .count()
    }

    // ---------------------------------------------------------------- canvas tools and drawing

    pub fn set_tool(&mut self, t: CanvasTool) {
        self.tool = t;
        self.pending = None;
    }

    /// Escape: drops a pending first point, else leaves the tool, else clears the selection and the selected operation.
    pub fn escape(&mut self) {
        if self.pending.is_some() {
            self.pending = None;
        } else if self.tool != CanvasTool::Select {
            self.tool = CanvasTool::Select;
        } else {
            self.clear_selection();
            self.selected_tab = None;
            self.selected_op = None;
        }
    }

    pub fn snap_at(&self, p: Point, radius: f64) -> Option<Snap> {
        snap::nearest(
            &self.snap_cache.cands,
            &self.project.items,
            |i| self.is_visible(i),
            p,
            radius,
        )
    }

    /// A primary click of the active tool at `p` (already snapped when `snapped`).
    pub fn canvas_click(&mut self, p: Point, snapped: bool) {
        match self.tool {
            CanvasTool::Select => {}
            CanvasTool::SetOrigin => {
                if snapped {
                    self.set_origin(WorkOrigin::Point(p));
                    self.tool = CanvasTool::Select;
                }
            }
            CanvasTool::Text => {
                self.draw.a = p;
                let shape = self.draw.shape(self.tool).expect("drawing tool");
                if let Err(e) = self.add_shape(shape) {
                    self.msg(Severity::Warning, e);
                }
            }
            CanvasTool::Line | CanvasTool::Rect | CanvasTool::Circle => {
                let Some(first) = self.pending.take() else {
                    self.pending = Some(p);
                    self.draw.a = p;
                    return;
                };
                let f = &mut self.draw;
                match self.tool {
                    CanvasTool::Line => (f.a, f.b) = (first, p),
                    CanvasTool::Rect => {
                        f.a = Point::new(first.x.min(p.x), first.y.min(p.y));
                        (f.w, f.h) = ((p.x - first.x).abs(), (p.y - first.y).abs());
                    }
                    _ => (f.a, f.d) = (first, 2.0 * first.distance(p)),
                }
                let shape = self.draw.shape(self.tool).expect("drawing tool");
                if let Err(e) = self.add_shape(shape) {
                    self.msg(Severity::Warning, e);
                }
            }
        }
    }

    /// Validates and adds the shape on layer Drawing; the new items become the selection.
    /// Text: characters not in the font are skipped and reported as a Warning, the rest is added.
    pub fn add_shape(&mut self, s: Shape) -> Result<Vec<u64>, String> {
        let contour = match s {
            Shape::Text {
                at,
                text,
                font,
                height,
            } => {
                if !(height.is_finite() && height > 0.0) {
                    return Err("Text needs a height".into());
                }
                let g = hc_text::text(&text, font, height, at).map_err(|e| e.to_string())?;
                if !g.missing.is_empty() {
                    let chars: Vec<String> = g.missing.iter().map(|&c| visible_char(c)).collect();
                    self.msg(
                        Severity::Warning,
                        format!("Not in the font: {}", chars.join(", ")),
                    );
                }
                return Ok(self.add_geoms(g.contours.into_iter().map(Geom::Contour).collect()));
            }
            Shape::Line { a, b } => shapes::line(a, b).ok_or("Line is too short")?,
            Shape::Rect { min, w, h, r } => {
                if !(w >= EPS && h >= EPS && w.is_finite() && h.is_finite()) {
                    return Err("Rectangle needs a width and a height".into());
                }
                if r.is_nan() || r < 0.0 {
                    return Err("Corner radius must be 0 or more".into());
                }
                let half = w.min(h) / 2.0;
                shapes::rect(min, w, h, r).ok_or_else(|| {
                    format!(
                        "Corner radius must be at most {}",
                        fmt_len(half, self.project.units)
                    )
                })?
            }
            Shape::Circle { c, r } => shapes::circle(c, r).ok_or("Circle needs a diameter")?,
        };
        Ok(self.add_geoms(vec![Geom::Contour(contour)]))
    }

    /// Adds drawn geometry; the new items become the selection (and the current thing). Nothing (not dirty) when empty.
    fn add_geoms(&mut self, geoms: Vec<Geom>) -> Vec<u64> {
        if geoms.is_empty() {
            return Vec::new();
        }
        let ids = self.project.add_drawn(geoms);
        self.items_changed();
        self.selection = ids.iter().copied().collect();
        self.selected_op = None;
        self.selected_tab = None;
        self.dirty = true;
        ids
    }

    /// Union of the selected items' bboxes (empty when nothing is selected).
    pub fn selection_bbox(&self) -> BBox {
        self.project
            .items
            .iter()
            .zip(&self.item_bbox)
            .filter(|(i, _)| self.selection.contains(&i.id))
            .fold(BBox::empty(), |acc, (_, b)| acc.union(b))
    }

    pub fn selection_center(&self) -> Point {
        let b = self.selection_bbox();
        Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
    }

    /// No-op on an empty selection or a degenerate / non-finite transform (it would collapse the part).
    pub fn transform_selection(&mut self, t: &Transform) {
        let finite = [t.a, t.b, t.c, t.d, t.tx, t.ty]
            .iter()
            .all(|v| v.is_finite());
        if self.selection.is_empty() || !finite || (t.a * t.d - t.b * t.c).abs() <= 1e-12 {
            return;
        }
        let ids: Vec<u64> = self.selection.iter().copied().collect();
        self.project.transform_items(&ids, t);
        self.items_changed();
        self.dirty = true;
    }

    /// Rotate by the form angle about the selection centre (Transform button and sidebar).
    pub fn rotate_selection(&mut self) {
        let t = Transform::rotate(self.xform.angle_deg.to_radians(), self.selection_center());
        self.transform_selection(&t);
    }

    /// Flip about the selection centre: horizontally = mirror across the vertical centre line.
    pub fn flip_selection(&mut self, horizontal: bool) {
        let c = self.selection_center();
        let t = if horizontal {
            Transform::mirror_y(c.x)
        } else {
            Transform::mirror_x(c.y)
        };
        self.transform_selection(&t);
    }

    /// Removes the selected items and their op references / manual tabs.
    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        for id in std::mem::take(&mut self.selection) {
            self.project.remove_item(id);
        }
        self.selected_tab = None;
        self.items_changed();
        self.dirty = true;
    }

    // ---------------------------------------------------------------- undo / redo

    /// Records the project as one undo step when it changed since the last commit and the user is idle
    /// (no button down, no text focus), so a drag or typing is one step.
    pub fn commit_undo(&mut self, idle: bool) {
        if !idle || self.project == self.undo_base {
            return;
        }
        let prev = std::mem::replace(&mut self.undo_base, self.project.clone());
        self.undo.push(prev);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.project != self.undo_base
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.commit_undo(true);
        let Some(p) = self.undo.pop() else {
            return false;
        };
        let cur = std::mem::replace(&mut self.project, p);
        self.redo.push(cur);
        self.history_moved();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(p) = self.redo.pop() else {
            return false;
        };
        let cur = std::mem::replace(&mut self.project, p);
        self.undo.push(cur);
        self.history_moved();
        true
    }

    fn history_moved(&mut self) {
        self.undo_base = self.project.clone();
        self.items_changed();
        self.selected_tab = None;
        let ops = &self.project.operations;
        self.selected_op = self
            .selected_op
            .filter(|id| ops.iter().any(|o| o.id == *id));
        self.pending = None;
        self.dirty = self.project != self.saved;
    }

    // ---------------------------------------------------------------- operations

    /// Project copies first, then library tools whose id the project does not have; sorted by id.
    pub fn available_tools(&self) -> Vec<Tool> {
        let mut v = self.project.tools.clone();
        for t in &self.library.tools {
            if !v.iter().any(|p| p.id == t.id) {
                v.push(t.clone());
            }
        }
        v.sort_by_key(|t| t.id);
        v
    }

    pub fn tool(&self, id: u32) -> Option<Tool> {
        self.available_tools().into_iter().find(|t| t.id == id)
    }

    /// New operation on the current selection; None if the selection is empty or the tool is unknown.
    pub fn new_operation(&mut self, kind: NewKind, tool_id: u32) -> Option<u64> {
        if self.selection.is_empty() {
            return None;
        }
        let tool = self.tool(tool_id)?;
        let n = self
            .project
            .operations
            .iter()
            .filter(|o| kind_label(&o.kind) == kind.label())
            .count()
            + 1;
        let id = self.project.add_operation(
            &format!("{} {n}", kind.label()),
            kind.op_kind(),
            &tool,
            self.selection.iter().copied().collect(),
        );
        self.selected_op = Some(id);
        self.selected_tab = None;
        self.dirty = true;
        Some(id)
    }

    pub fn move_op(&mut self, id: u64, dir: MoveDir) {
        let ops = &mut self.project.operations;
        let Some(i) = ops.iter().position(|o| o.id == id) else {
            return;
        };
        let j = match dir {
            MoveDir::Up if i > 0 => i - 1,
            MoveDir::Down if i + 1 < ops.len() => i + 1,
            _ => return,
        };
        ops.swap(i, j);
        self.dirty = true;
    }

    pub fn delete_op(&mut self, id: u64) {
        self.project.remove_operation(id);
        if self.selected_op == Some(id) {
            self.selected_op = None;
            self.selected_tab = None;
        }
        self.dirty = true;
    }

    /// Makes the op current and selects its items.
    pub fn select_op(&mut self, id: u64) {
        if let Some(op) = self.project.operations.iter().find(|o| o.id == id) {
            self.selection = op.items.iter().copied().collect();
            if self.selected_op != Some(id) {
                self.selected_tab = None;
            }
            self.selected_op = Some(id);
        }
    }

    /// Replaces the op with an edited copy; a changed tool is embedded.
    pub fn update_op(&mut self, op: hc_project::Operation) {
        let Some(slot) = self.project.operations.iter_mut().find(|o| o.id == op.id) else {
            return;
        };
        if *slot == op {
            return;
        }
        let tool_changed = slot.tool != op.tool;
        *slot = op.clone();
        if tool_changed {
            self.embed_tool(op.tool);
        }
        self.dirty = true;
    }

    fn embed_tool(&mut self, id: u32) {
        if self.project.tools.iter().any(|t| t.id == id) {
            return;
        }
        if let Some(t) = self.library.get(id).cloned() {
            self.project.tools.push(t);
        }
    }

    pub fn apply_tool_defaults(&mut self, op_id: u64) {
        let Some(op) = self.project.operations.iter().find(|o| o.id == op_id) else {
            return;
        };
        let Some(t) = self.tool(op.tool) else {
            return;
        };
        let mut op = op.clone();
        op.depth_per_pass = t.depth_per_pass;
        op.feed = t.feed;
        op.plunge_feed = t.plunge_feed;
        op.rpm = t.rpm;
        if let OpKind::Pocket { stepover_pct } = &mut op.kind {
            *stepover_pct = t.stepover_pct;
        }
        self.update_op(op);
    }

    pub fn use_selection(&mut self, op_id: u64) {
        if let Some(op) = self.project.operations.iter().find(|o| o.id == op_id) {
            let mut op = op.clone();
            op.items = self.selection.iter().copied().collect();
            self.update_op(op);
        }
    }

    // ---------------------------------------------------------------- tabs

    /// The selected op when it is a Contour with tabs enabled.
    fn tab_op(&self) -> Option<&Operation> {
        let id = self.selected_op?;
        self.project
            .operations
            .iter()
            .find(|o| o.id == id && o.tabs.enabled && matches!(o.kind, OpKind::Contour { .. }))
    }

    /// Closed contour of `item` when it belongs to `op`.
    fn tab_item<'a>(&'a self, op: &Operation, item: u64) -> Option<&'a Contour> {
        if !op.items.contains(&item) {
            return None;
        }
        match &self.item(item)?.geom {
            Geom::Contour(c) if c.closed && c.length() > 0.0 => Some(c),
            _ => None,
        }
    }

    /// Tab positions (fractions) per closed item of the selected tab op, as the toolpath uses them.
    pub fn tab_markers(&self) -> Vec<(u64, &Contour, Vec<f64>)> {
        let Some(op) = self.tab_op() else {
            return Vec::new();
        };
        let d = self.tool(op.tool).map_or(0.0, |t| t.diameter);
        op.items
            .iter()
            .filter_map(|&id| {
                let c = self.tab_item(op, id)?;
                Some((id, c, hc_toolpath::tab_positions(c, &op.tabs, id, d).0))
            })
            .collect()
    }

    /// Nearest tab marker within `tol` of `p`.
    pub fn tab_at(&self, p: Point, tol: f64) -> Option<(u64, usize)> {
        let mut best = None;
        let mut best_d = tol;
        for (id, c, pos) in self.tab_markers() {
            let l = c.length();
            for (i, t) in pos.iter().enumerate() {
                let d = c.point_at(t.rem_euclid(1.0) * l).distance(p);
                if d <= best_d {
                    best_d = d;
                    best = Some((id, i));
                }
            }
        }
        best
    }

    /// Edits the manual positions of `item` on the selected tab op (materializing the current ones first).
    /// `f` gets the positions and the fraction of `p` on the item.
    fn edit_tabs(&mut self, item: u64, p: Point, f: impl FnOnce(&mut Vec<f64>, f64)) {
        let Some(op) = self.tab_op() else { return };
        let Some(c) = self.tab_item(op, item) else {
            return;
        };
        let t = c.closest(p).0 / c.length();
        let mut op = op.clone();
        if !op.tabs.manual.iter().any(|m| m.item == item) {
            let d = self.tool(op.tool).map_or(0.0, |t| t.diameter);
            let positions = hc_toolpath::tab_positions(c, &op.tabs, item, d).0;
            op.tabs.manual.push(ManualTabs { item, positions });
        }
        let m = op
            .tabs
            .manual
            .iter_mut()
            .find(|m| m.item == item)
            .expect("just added");
        f(&mut m.positions, t);
        self.update_op(op);
    }

    pub fn add_tab(&mut self, item: u64, p: Point) {
        let mut added = None;
        self.edit_tabs(item, p, |v, t| {
            v.push(t);
            added = Some(v.len() - 1);
        });
        if let Some(i) = added {
            self.selected_tab = Some((item, i));
        }
    }

    /// True when `idx` is a current tab of `item` on the selected tab op.
    fn has_tab(&self, item: u64, idx: usize) -> bool {
        self.tab_markers()
            .iter()
            .any(|(id, _, pos)| *id == item && idx < pos.len())
    }

    pub fn move_tab(&mut self, item: u64, idx: usize, p: Point) {
        if !self.has_tab(item, idx) {
            return;
        }
        self.edit_tabs(item, p, |v, t| {
            if let Some(x) = v.get_mut(idx) {
                *x = t;
            }
        });
    }

    pub fn delete_tab(&mut self, item: u64, idx: usize) {
        if !self.has_tab(item, idx) {
            return;
        }
        let mut deleted = false;
        self.edit_tabs(item, Point::new(0.0, 0.0), |v, _| {
            if idx < v.len() {
                v.remove(idx);
                deleted = true;
            }
        });
        if let Some((sel_item, sel)) = self.selected_tab.filter(|&(i, _)| deleted && i == item) {
            self.selected_tab = match sel.cmp(&idx) {
                std::cmp::Ordering::Less => Some((sel_item, sel)),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some((sel_item, sel - 1)),
            };
        }
    }

    /// "Reset to auto": drops every manual entry of `op` (the selected tab op only).
    pub fn reset_tabs(&mut self, op: u64) {
        let Some(o) = self.tab_op().filter(|o| o.id == op) else {
            return;
        };
        let mut o = o.clone();
        o.tabs.manual.clear();
        self.update_op(o);
        self.selected_tab = None;
    }

    // ---------------------------------------------------------------- tool library

    /// Library edits are allowed only when they can be saved (loaded fine, path known).
    pub fn library_can_save(&self) -> bool {
        self.library_error.is_none() && self.library_path.is_some()
    }

    /// "Start a new empty library" after a load error; the broken file becomes tools.json.bak on the first save.
    pub fn start_new_library(&mut self) {
        self.library = ToolLibrary::default();
        self.library_error = None;
    }

    /// Writes the library (atomic, old file kept as .bak by hc-project); creates the directory.
    pub fn save_library(&mut self) -> bool {
        let Some(path) = self
            .library_path
            .clone()
            .filter(|_| self.library_can_save())
        else {
            return false;
        };
        let res = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| self.library.save(&path).map_err(|e| e.to_string()));
        match res {
            Ok(()) => true,
            Err(e) => {
                self.msg(Severity::Error, format!("Cannot save tool library: {e}"));
                false
            }
        }
    }

    /// Applies one committed library edit and saves at once. A rejected edit changes nothing;
    /// a failed save keeps the edit in memory and shows the error.
    fn edit_library(
        &mut self,
        f: impl FnOnce(&mut ToolLibrary) -> Result<(), hc_project::ProjectError>,
    ) -> bool {
        if !self.library_can_save() {
            return false;
        }
        if let Err(e) = f(&mut self.library) {
            self.msg(Severity::Error, format!("Tool library: {e}"));
            return false;
        }
        self.save_library()
    }

    pub fn library_add(&mut self, t: Tool) -> bool {
        self.edit_library(|l| l.add(t))
    }

    pub fn library_update(&mut self, t: Tool) -> bool {
        self.edit_library(|l| l.update(t))
    }

    pub fn library_remove(&mut self, id: u32) -> bool {
        self.edit_library(|l| {
            l.remove(id)
                .map(|_| ())
                .ok_or(hc_project::ProjectError::NoSuchTool(id))
        })
    }

    /// ("Error" | "Warning", "<op name or Project> · <message>") of an issue.
    pub fn issue_parts(&self, i: &Issue) -> (&'static str, String) {
        let word = match i.severity {
            hc_toolpath::Severity::Error => "Error",
            hc_toolpath::Severity::Warning => "Warning",
        };
        let place = match i.op {
            None => "Project".to_string(),
            Some(id) => self
                .project
                .operations
                .iter()
                .find(|o| o.id == id)
                .map_or(format!("Operation {id}"), |o| o.name.clone()),
        };
        (word, format!("{place} · {}", i.message))
    }

    /// "Error · Contour 1 · Spindle speed is 0 (S0)" (tests only).
    #[cfg(test)]
    pub fn issue_text(&self, i: &Issue) -> String {
        let (word, rest) = self.issue_parts(i);
        format!("{word} · {rest}")
    }

    /// "1 error, 2 warnings" for an op and whether it has errors; None when clean.
    pub fn op_issue_summary(&self, op: u64) -> Option<(String, bool)> {
        let count = |sev| {
            self.issues
                .iter()
                .filter(|i| i.op == Some(op) && i.severity == sev)
                .count()
        };
        let (e, w) = (
            count(hc_toolpath::Severity::Error),
            count(hc_toolpath::Severity::Warning),
        );
        let part = |n: usize, word: &str| match n {
            0 => None,
            1 => Some(format!("1 {word}")),
            n => Some(format!("{n} {word}s")),
        };
        let parts: Vec<String> = [part(e, "error"), part(w, "warning")]
            .into_iter()
            .flatten()
            .collect();
        (!parts.is_empty()).then(|| (parts.join(", "), e > 0))
    }

    /// Issue row click: the op becomes current; an item issue selects just that item.
    pub fn select_issue(&mut self, i: &Issue) {
        if let Some(op) = i.op {
            self.select_op(op);
            if let Some(item) = i.item {
                self.selection = [item].into_iter().collect();
            }
        }
    }

    // ---------------------------------------------------------------- properties

    /// (count, length, segment count) etc. for the single-item properties view.
    pub fn item(&self, id: u64) -> Option<&Item> {
        self.project.items.iter().find(|i| i.id == id)
    }
}

/// A missing character for the warning; one the UI font (Ubuntu Light) cannot show either, and control characters, as U+XXXX.
fn visible_char(c: char) -> String {
    let shown = hc_text::text(
        &c.to_string(),
        hc_text::Font::Outline,
        1.0,
        Point::new(0.0, 0.0),
    )
    .is_ok_and(|g| g.missing.is_empty());
    if shown {
        c.to_string()
    } else {
        format!("U+{:04X}", c as u32)
    }
}

pub fn warning_text(w: &ImportWarning) -> String {
    match w {
        ImportWarning::UnitsAssumedMm { insunits } => {
            format!("Drawing units unknown ($INSUNITS = {insunits}), assumed millimetres")
        }
        ImportWarning::UnsupportedExtrusion { entity } => {
            format!("{entity} is not in the XY plane and was skipped")
        }
        ImportWarning::PaperSpaceSkipped { count } => {
            format!("{count} paper space entities skipped (only model space is imported)")
        }
        ImportWarning::MissingBlock { name } => format!("Block \"{name}\" is missing"),
        ImportWarning::InsertArrayFirstOnly { block } => {
            format!("Block \"{block}\" is an array insert; only the first copy was imported")
        }
        ImportWarning::SplineFitPointsOnly { layer } => {
            format!("A spline on layer \"{layer}\" has only fit points; it was approximated")
        }
        ImportWarning::InsertNestingTooDeep { block } => {
            format!("Block \"{block}\" is nested too deep and was skipped")
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use hc_project::ToolKind;

    pub fn bracket_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/dxf/autocad2018_bracket_inch.dxf")
    }

    pub fn bracket_project() -> Project {
        let imp = hc_dxf::import_dxf_file(&bracket_path(), &ImportOptions::default()).unwrap();
        let mut p = Project::new();
        p.add_dxf(&imp, "bracket.dxf");
        p
    }

    /// Fresh scratch dir under the system temp dir (never the real config dir).
    pub fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("hobbycam-app-test-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn tool(id: u32) -> Tool {
        Tool {
            id,
            name: format!("Flat {id}"),
            kind: ToolKind::FlatEndMill,
            diameter: 3.175,
            flute_length: 12.0,
            flutes: 2,
            feed: 600.0,
            plunge_feed: 150.0,
            rpm: 16000,
            depth_per_pass: 0.8,
            stepover_pct: 45.0,
            comment: String::new(),
        }
    }

    /// Outline circle ids with diameter `d` (mm).
    pub fn circles(s: &AppState, d: f64) -> Vec<u64> {
        s.project
            .items
            .iter()
            .filter(|i| i.layer == "Outline")
            .filter(|i| match &i.geom {
                Geom::Contour(c) => c
                    .as_circle()
                    .is_some_and(|ci| (2.0 * ci.radius - d).abs() < 1e-6),
                _ => false,
            })
            .map(|i| i.id)
            .collect()
    }

    /// The largest closed Outline contour (the bracket's outer edge).
    pub fn outer(s: &AppState) -> u64 {
        s.project
            .items
            .iter()
            .filter(|i| i.layer == "Outline")
            .filter_map(|i| match &i.geom {
                Geom::Contour(c) if c.closed => Some((i.id, c.signed_area().abs())),
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
            .0
    }

    /// Centre of the Bracket's Ø76.2 circle (walkthrough step 3 picks its Center snap).
    pub fn pocket_center(s: &AppState) -> Point {
        let id = circles(s, 76.2)[0];
        match &s.item(id).unwrap().geom {
            Geom::Contour(c) => c.as_circle().unwrap().center,
            Geom::Point(_) => unreachable!("a circle"),
        }
    }

    /// Walkthrough steps 2–3: the real 220 × 140 mm blank at X 17, Y 19 around the bracket, and the origin at
    /// the Ø76.2 circle's centre. Returns that centre.
    pub fn walkthrough_stock_origin(s: &mut AppState) -> Point {
        let mut st = s.project.stock.clone();
        (st.min_x, st.min_y, st.width, st.height) = (17.0, 19.0, 220.0, 140.0);
        let b = match &s.item(outer(s)).unwrap().geom {
            Geom::Contour(c) => c.bbox(),
            Geom::Point(_) => unreachable!("a contour"),
        };
        assert!(b.min.x > st.min_x && b.min.y > st.min_y);
        assert!(b.max.x < st.min_x + st.width && b.max.y < st.min_y + st.height);
        s.set_stock(st);
        let c = pocket_center(s);
        s.set_origin(WorkOrigin::Point(c));
        c
    }

    fn op_on(s: &mut AppState, items: Vec<u64>, kind: NewKind, tool: u32) -> u64 {
        s.selection = items.into_iter().collect();
        s.new_operation(kind, tool).unwrap()
    }

    pub fn edit_op(s: &mut AppState, id: u64, f: impl FnOnce(&mut Operation)) {
        let mut o = s
            .project
            .operations
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .clone();
        f(&mut o);
        s.update_op(o);
    }

    /// M8 test 28: Outside (tabs), Pocket, Drill on the Bracket; returns (state, outside op).
    pub fn bracket_clean() -> (AppState, u64) {
        let mut s = AppState::new(None);
        s.library.add(tool(1)).unwrap();
        let mut t2 = tool(2);
        t2.name = "Drill 5/8".into();
        t2.kind = ToolKind::Drill;
        t2.diameter = 15.875;
        t2.flute_length = 20.0;
        t2.plunge_feed = 100.0;
        t2.feed = 100.0;
        t2.rpm = 3000;
        s.library.add(t2).unwrap();
        s.open_cli(&bracket_path());
        let edge = outer(&s);
        let outside = op_on(&mut s, vec![edge], NewKind::Contour, 1);
        edit_op(&mut s, outside, |o| {
            o.depth = 10.0;
            o.tabs.enabled = true;
            o.tabs.placement = hc_project::TabPlacement::Count { count: 4 };
            o.tabs.width = 5.0;
            o.tabs.height = 2.0;
        });
        let pocket = [circles(&s, 76.2), circles(&s, 44.45)].concat();
        let p = op_on(&mut s, pocket, NewKind::Pocket, 1);
        edit_op(&mut s, p, |o| o.depth = 3.0);
        let holes = circles(&s, 15.875);
        let d = op_on(&mut s, holes, NewKind::Drill, 2);
        edit_op(&mut s, d, |o| o.depth = 10.0);
        s.clear_selection();
        (s, outside)
    }

    /// M8 test 32: test 28 plus an Inside op with a Ø20 tool on a Ø15.875 hole, and rpm 0 on the Outside op.
    /// Returns (state, inside op).
    pub fn bracket_issues() -> (AppState, u64) {
        let (mut s, outside) = bracket_clean();
        let mut t3 = tool(3);
        t3.diameter = 20.0;
        s.library.add(t3).unwrap();
        let hole = circles(&s, 15.875)[0];
        let inside = op_on(&mut s, vec![hole], NewKind::Contour, 3);
        edit_op(&mut s, inside, |o| {
            o.kind = OpKind::Contour {
                side: Side::Inside,
                direction: Direction::Climb,
            }
        });
        edit_op(&mut s, outside, |o| o.rpm = 0);
        s.clear_selection();
        s.selected_op = None;
        (s, inside)
    }

    #[test]
    fn m8_t28_bracket_clean() {
        let (mut s, _) = bracket_clean();
        s.refresh_toolpaths();
        assert_eq!(s.project.operations.len(), 3);
        assert!(s.toolpaths.iter().all(|(_, r)| r.is_ok()));
        assert_eq!(s.issues, []);
    }

    #[test]
    fn m8_t31_issues_live_and_on_open() {
        let (mut s, outside) = bracket_clean();
        s.refresh_toolpaths();
        assert!(s.issues.is_empty());
        edit_op(&mut s, outside, |o| o.rpm = 0);
        s.refresh_toolpaths();
        let rules: Vec<_> = s.issues.iter().map(|i| (i.rule, i.op)).collect();
        assert_eq!(rules, [(hc_toolpath::RuleId::SpindleOff, Some(outside))]);
        assert_eq!(
            s.issue_text(&s.issues[0]),
            "Warning · Contour 1 · Spindle speed is 0 (S0)"
        );
        assert_eq!(
            s.op_issue_summary(outside),
            Some(("1 warning".to_string(), false))
        );

        // a dangling item: no Warning message on open, an issue instead
        edit_op(&mut s, outside, |o| o.items.push(999));
        let path = scratch("m8_t31").join("dangling.hcam");
        assert!(s.save_as(&path));
        let mut s = AppState::new(None);
        let n = s.messages.len();
        assert!(s.open(&path));
        assert_eq!(s.messages.len(), n);
        s.refresh_toolpaths();
        assert!(s
            .issues
            .iter()
            .any(|i| i.rule == hc_toolpath::RuleId::MissingItem && i.item == Some(999)));
        assert_eq!(
            s.op_issue_summary(outside),
            Some(("1 error, 1 warning".to_string(), true))
        );

        // clicking an item issue selects the op and just that item; a project issue only shows
        let i = s.issues[0].clone();
        s.select_issue(&i);
        assert_eq!(s.selected_op, Some(outside));
        assert_eq!(s.selection.iter().copied().collect::<Vec<_>>(), [999]);
        s.clear_selection();
        s.selected_op = None;
        s.select_issue(&hc_toolpath::Issue { op: None, ..i });
        assert_eq!(s.selected_op, None);
    }

    #[test]
    fn t11_app_state() {
        let mut s = AppState::new(Some(scratch("t11").join("tools.json")));
        assert!(s.library_error.is_none());
        s.library.add(tool(1)).unwrap();
        assert!(s.import_dxf(&bracket_path()));
        assert!(s.dirty);
        assert_eq!(s.project.layers.len(), 4);
        assert_eq!(s.project.items.len(), 29);
        let b = s.items_bbox();
        assert!(b.min.approx_eq(Point::new(-12.7, -12.7), 1e-6));
        assert!(b.max.approx_eq(Point::new(457.2, 190.5), 1e-6));
        assert!(s.fit_pending);
        assert!(s
            .messages
            .iter()
            .any(|m| m.text.contains("Ignored: 6 DIMENSION, 4 HATCH")));

        assert_eq!(s.new_operation(NewKind::Pocket, 1), None); // empty selection
        let first = s.project.items[0].id;
        s.selection.insert(first);
        let a = s.new_operation(NewKind::Pocket, 1).unwrap();
        let op = &s.project.operations[0];
        assert_eq!(
            (op.name.as_str(), op.tool, &op.items),
            ("Pocket 1", 1, &vec![first])
        );
        assert_eq!(op.kind, OpKind::Pocket { stepover_pct: 45.0 });
        let b = s.new_operation(NewKind::Drill, 1).unwrap();
        let order = |s: &AppState| {
            s.project
                .operations
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>()
        };
        s.move_op(a, MoveDir::Up);
        assert_eq!(order(&s), [a, b]);
        s.move_op(b, MoveDir::Down);
        assert_eq!(order(&s), [a, b]);
        s.move_op(b, MoveDir::Up);
        assert_eq!(order(&s), [b, a]);
        s.move_op(b, MoveDir::Down);
        assert_eq!(order(&s), [a, b]);

        let bad = scratch("t11").join("corrupt.hcam");
        std::fs::write(&bad, "{ not json").unwrap();
        let before = s.project.clone();
        let n = s.messages.len();
        assert!(!s.open(&bad));
        assert_eq!(s.project, before);
        assert_eq!(s.messages.len(), n + 1);
        assert_eq!(s.messages.last().unwrap().severity, Severity::Error);
    }

    /// M5 acceptance test 16.
    #[test]
    fn m5_t16_stock_fit_and_auto_regeneration() {
        let mut s = AppState::new(None);
        assert!(s.import_dxf(&bracket_path()));
        let st = &s.project.stock;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(st.min_x, -12.7) && near(st.min_y, -12.7));
        assert!(near(st.width, 469.9) && near(st.height, 203.2));
        assert_eq!(st.thickness, 10.0);
        assert_eq!(st.origin, Project::new().stock.origin);

        // edited stock: a second import into a new project keeps it
        let mut s2 = AppState::new(None);
        s2.project.stock.thickness = 12.0;
        let edited = s2.project.stock.clone();
        s2.import_dxf(&bracket_path());
        assert_eq!(s2.project.stock, edited);
        // an operation present: unchanged as well
        let mut s3 = AppState::new(None);
        s3.library.add(tool(1)).unwrap();
        s3.import_dxf(&bracket_path());
        s3.project.stock = Project::new().stock;
        s3.selection.insert(1);
        s3.new_operation(NewKind::Engrave, 1).unwrap();
        s3.import_dxf(&bracket_path());
        assert_eq!(s3.project.stock, Project::new().stock);

        // auto regeneration
        s.library.add(tool(1)).unwrap();
        s.refresh_toolpaths();
        assert!(s.toolpaths.is_empty());
        let item = s
            .project
            .items
            .iter()
            .find(|i| matches!(i.geom, hc_project::Geom::Contour(_)))
            .unwrap()
            .id;
        s.selection.insert(item);
        let id = s.new_operation(NewKind::Engrave, 1).unwrap();
        s.refresh_toolpaths();
        assert_eq!(s.toolpaths.len(), 1);
        assert_eq!(s.toolpaths[0].0, id);
        let min_z = |s: &AppState| {
            let tp = s.toolpath_result(id).unwrap().as_ref().unwrap();
            tp.moves
                .iter()
                .filter_map(|m| match m {
                    hc_toolpath::Move::Linear { to, .. } => Some(to.z),
                    _ => None,
                })
                .fold(f64::INFINITY, f64::min)
        };
        assert!((min_z(&s) + 0.8).abs() < 1e-9);
        let mut op = s.project.operations[0].clone();
        op.depth = 2.5;
        s.update_op(op.clone());
        s.refresh_toolpaths();
        assert!((min_z(&s) + 2.5).abs() < 1e-9);
        op.enabled = false;
        s.update_op(op);
        s.refresh_toolpaths();
        assert!(s.toolpaths.is_empty());
    }

    #[test]
    fn t9_hiding_layer_drops_selection() {
        let mut s = AppState::new(None);
        s.import_dxf(&bracket_path());
        s.selection = s.project.items.iter().map(|i| i.id).collect();
        s.set_layer_visible("Outline", false);
        assert_eq!(s.selection.len(), 29 - 12);
        assert!(s
            .selection
            .iter()
            .all(|id| s.item(*id).unwrap().layer != "Outline"));
    }

    #[test]
    fn click_and_box_selection() {
        let mut s = AppState::new(None);
        s.import_dxf(&bracket_path());
        let hole = Point::new(56.4126, 56.4126);
        s.click_select(hole, 1.5, false);
        assert_eq!(s.selection.len(), 1);
        s.click_select(hole, 1.5, true);
        assert!(s.selection.is_empty());
        let (a, b) = (Point::new(22.86, 22.86), Point::new(332.74, 154.94));
        s.box_select(a, b, false, 0.1);
        assert_eq!(s.selection.len(), 23);
        // dragged right-to-left = crossing
        s.box_select(Point::new(b.x, a.y), Point::new(a.x, b.y), false, 0.1);
        assert_eq!(s.selection.len(), 24);
        s.click_select(Point::new(500.0, 500.0), 1.5, false);
        assert!(s.selection.is_empty());
    }

    #[test]
    fn save_open_round_trip_and_cli() {
        let dir = scratch("roundtrip");
        let mut s = AppState::new(None);
        s.open_cli(&dir.join("x.DXF")); // missing file: error message, still running
        assert!(s.messages.last().unwrap().text.contains("Cannot import"));
        s.open_cli(&dir.join("x.txt"));
        assert!(s.messages.last().unwrap().text.contains("expected a .dxf"));
        s.open_cli(&bracket_path());
        assert_eq!(s.project.items.len(), 29);
        assert!(!s.save()); // no path yet
        assert!(s.dirty);
        let f = dir.join("p.hcam");
        assert!(s.save_as(&f));
        assert!(!s.dirty);
        assert!(s.title().ends_with("p.hcam"));
        s.set_units(Units::Inch);
        assert!(s.title().ends_with('*'));
        let mut t = AppState::new(None);
        t.open_cli(&f);
        assert_eq!(t.project.items, s.project.items);
        // failed save keeps dirty
        assert!(!s.save_as(&dir.join("missing-dir").join("p.hcam")));
        assert!(s.dirty);
    }

    #[test]
    fn library_errors_and_backup() {
        let dir = scratch("lib");
        let path = dir.join("cfg").join("tools.json");
        // missing dir is created on save
        let mut s = AppState::new(Some(path.clone()));
        assert!(s.library_add(tool(3))); // auto-saved
        assert_eq!(AppState::new(Some(path.clone())).library.tools.len(), 1);
        let mut t = tool(3);
        t.name = "edited".into();
        assert!(s.library_update(t));
        assert_eq!(
            AppState::new(Some(path.clone())).library.tools[0].name,
            "edited"
        );
        assert!(!s.library_add(tool(3)), "duplicate id rejected");
        assert_eq!(s.library.tools.len(), 1);
        assert!(s.library_remove(3));
        assert!(AppState::new(Some(path.clone())).library.tools.is_empty());
        // corrupt file: error, edits refused, file untouched until "start new"
        std::fs::write(&path, "garbage").unwrap();
        let mut s = AppState::new(Some(path.clone()));
        assert!(s.library_error.is_some());
        assert!(!s.library_add(tool(1)));
        assert!(s.library.tools.is_empty());
        assert!(!s.save_library());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "garbage");
        s.start_new_library();
        assert!(s.library_add(tool(1)));
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            "garbage"
        );
        assert!(AppState::new(Some(path)).library_error.is_none());
    }

    #[test]
    fn library_failed_save_keeps_edit() {
        let dir = scratch("lib_fail");
        // the "directory" is a file, so create_dir_all fails
        std::fs::write(dir.join("cfg"), "x").unwrap();
        let mut s = AppState::new(Some(dir.join("cfg").join("tools.json")));
        // reading through a file path fails on Unix; Windows reports NotFound, i.e. "no library yet"
        #[cfg(unix)]
        assert!(s.library_error.is_some());
        s.start_new_library();
        let n = s.messages.len();
        assert!(!s.library_add(tool(1)));
        assert_eq!(s.library.tools.len(), 1, "edit kept in memory");
        assert_eq!(s.messages.len(), n + 1);
        assert_eq!(s.messages.last().unwrap().severity, Severity::Error);
    }

    #[test]
    fn op_edits() {
        let mut s = AppState::new(None);
        s.library.add(tool(1)).unwrap();
        let mut t2 = tool(2);
        t2.feed = 999.0;
        s.library.add(t2).unwrap();
        s.import_dxf(&bracket_path());
        s.selection.insert(s.project.items[0].id);
        let id = s.new_operation(NewKind::Contour, 1).unwrap();
        s.clear_selection();
        s.select_op(id);
        assert_eq!(s.selection.len(), 1);
        let mut op = s.project.operations[0].clone();
        op.tool = 2;
        s.update_op(op);
        assert!(s.project.tools.iter().any(|t| t.id == 2));
        s.apply_tool_defaults(id);
        assert_eq!(s.project.operations[0].feed, 999.0);
        s.selection.insert(s.project.items[1].id);
        s.use_selection(id);
        assert_eq!(s.project.operations[0].items.len(), 2);
        s.delete_op(id);
        assert!(s.project.operations.is_empty() && s.selected_op.is_none());
    }
}

#[cfg(test)]
mod edge_tests {
    use super::tests::{bracket_clean, bracket_path, edit_op, scratch, tool};
    use super::*;
    use hc_project::Geom;

    fn loaded(lib_tools: &[u32]) -> AppState {
        let mut s = AppState::new(None);
        for &id in lib_tools {
            s.library.add(tool(id)).unwrap();
        }
        assert!(s.import_dxf(&bracket_path()));
        s
    }

    #[test]
    fn import_twice_ids_keep_increasing() {
        let mut s = loaded(&[]);
        s.selection.insert(5);
        assert!(s.import_dxf(&bracket_path()));
        let ids: Vec<u64> = s.project.items.iter().map(|i| i.id).collect();
        assert_eq!(ids, (1..=58).collect::<Vec<u64>>());
        assert_eq!(s.project.layers.len(), 4);
        assert_eq!(s.item_bbox.len(), 58);
        assert_eq!(s.layer_item_count("Outline"), 24);
        assert!(s.selection.contains(&5)); // import does not drop the selection
        assert!(s.project.validate().is_empty());
    }

    #[test]
    fn delete_selected_and_other_ops() {
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        let a = s.new_operation(NewKind::Engrave, 1).unwrap();
        let b = s.new_operation(NewKind::Drill, 1).unwrap();
        assert_eq!(s.selected_op, Some(b));
        s.delete_op(a); // not the selected one
        assert_eq!(s.selected_op, Some(b));
        s.delete_op(b);
        assert_eq!(s.selected_op, None);
        assert!(s.project.operations.is_empty());
        // the canvas selection itself is untouched
        assert!(s.selection.contains(&1));
        // the op name counter does not reuse names of deleted ops' kinds badly: next is "Engrave 1"
        let c = s.new_operation(NewKind::Engrave, 1).unwrap();
        assert_eq!(s.project.operations[0].name, "Engrave 1");
        assert!(c > b); // ids never reused
    }

    #[test]
    fn no_op_edits_do_not_set_dirty() {
        let dir = scratch("noop_dirty");
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        let a = s.new_operation(NewKind::Engrave, 1).unwrap();
        let b = s.new_operation(NewKind::Engrave, 1).unwrap();
        assert!(s.save_as(&dir.join("p.hcam")));
        let order = |s: &AppState| {
            s.project
                .operations
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>()
        };
        s.move_op(a, MoveDir::Up);
        s.move_op(b, MoveDir::Down);
        s.move_op(999, MoveDir::Up);
        assert_eq!(order(&s), [a, b]);
        assert!(!s.dirty, "boundary moves are no-ops");
        let same = s.project.operations[0].clone();
        s.update_op(same);
        assert!(!s.dirty);
        s.set_units(Units::Mm);
        assert!(!s.dirty);
        // note: delete_op(unknown id) sets dirty; the UI only deletes listed ops, so not asserted
        s.delete_op(12345);
        assert_eq!(order(&s), [a, b]);
    }

    #[test]
    fn move_op_single_and_three() {
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        let a = s.new_operation(NewKind::Engrave, 1).unwrap();
        s.move_op(a, MoveDir::Up);
        s.move_op(a, MoveDir::Down);
        assert_eq!(s.project.operations.len(), 1);
        let b = s.new_operation(NewKind::Engrave, 1).unwrap();
        let c = s.new_operation(NewKind::Engrave, 1).unwrap();
        let order = |s: &AppState| {
            s.project
                .operations
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>()
        };
        s.move_op(c, MoveDir::Up);
        s.move_op(c, MoveDir::Up);
        assert_eq!(order(&s), [c, a, b]);
        s.move_op(c, MoveDir::Up);
        assert_eq!(order(&s), [c, a, b]);
        s.move_op(a, MoveDir::Down);
        s.move_op(a, MoveDir::Down);
        assert_eq!(order(&s), [c, b, a]);
    }

    #[test]
    fn op_on_hidden_layer_item_keeps_its_items() {
        let mut s = loaded(&[1]);
        let hidden_id = s
            .project
            .items
            .iter()
            .find(|i| i.layer == "Hidden")
            .unwrap()
            .id;
        s.selection = [1, hidden_id].into();
        let op = s.new_operation(NewKind::Engrave, 1).unwrap();
        s.set_layer_visible("Hidden", false);
        assert_eq!(s.selection, [1].into());
        // hiding never edits operations
        assert_eq!(s.project.operations[0].items, [1, hidden_id]);
        // selecting the op and pressing "Use current selection" must not drop the hidden item
        s.select_op(op);
        s.use_selection(op);
        assert_eq!(s.project.operations[0].items, [1, hidden_id]);
        assert!(s.project.validate().is_empty());
        // hidden items are not pickable
        let Geom::Contour(c) = &s.item(hidden_id).unwrap().geom else {
            panic!()
        };
        let on = c.segments[0].start();
        s.click_select(on, 1e-3, false);
        assert!(!s.selection.contains(&hidden_id));
        s.set_layer_visible("Hidden", true);
        s.click_select(on, 1e-3, false);
        assert_eq!(s.selection, [hidden_id].into());
    }

    #[test]
    fn set_units_round_trip_keeps_mm() {
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        s.new_operation(NewKind::Pocket, 1).unwrap();
        let before = s.project.clone();
        s.dirty = false;
        s.set_units(Units::Inch);
        assert!(s.dirty);
        assert_eq!(s.project.units, Units::Inch);
        assert_eq!(s.project.operations, before.operations);
        assert_eq!(s.project.items, before.items);
        assert_eq!(s.project.stock, before.stock);
        assert_eq!(s.project.tools, before.tools);
        s.set_units(Units::Mm);
        assert_eq!(s.project, before);
    }

    #[test]
    fn new_project_resets_everything() {
        let dir = scratch("newproj");
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        s.new_operation(NewKind::Engrave, 1).unwrap();
        s.set_layer_visible("Center", false);
        assert!(s.save_as(&dir.join("p.hcam")));
        s.set_units(Units::Inch);
        s.new_project();
        assert!(!s.dirty);
        assert!(s.selection.is_empty());
        assert_eq!(s.selected_op, None);
        assert!(s.hidden_layers.is_empty());
        assert_eq!(s.path, None);
        assert_eq!(s.project, Project::new());
        assert!(s.item_bbox.is_empty());
        assert!(s.fit_pending);
        assert!(s.title().ends_with("untitled"));
        // the library survives a new project
        assert!(s.library.get(1).is_some());
    }

    #[test]
    fn failed_open_keeps_everything() {
        let dir = scratch("failed_open");
        let mut s = loaded(&[1]);
        let good = dir.join("good.hcam");
        assert!(s.save_as(&good));
        s.selection = [1, 2].into();
        s.set_units(Units::Inch); // dirty
        s.set_layer_visible("Title", false);
        let snapshot = (
            s.project.clone(),
            s.path.clone(),
            s.dirty,
            s.selection.clone(),
            s.hidden_layers.clone(),
        );
        for bad in [
            dir.join("missing.hcam"),
            {
                let p = dir.join("v9.hcam");
                std::fs::write(&p, r#"{"format_version": 9}"#).unwrap();
                p
            },
            {
                let p = dir.join("empty.hcam");
                std::fs::write(&p, "").unwrap();
                p
            },
        ] {
            let n = s.messages.len();
            assert!(!s.open(&bad));
            assert_eq!(s.messages.len(), n + 1);
            assert!(s.messages.last().unwrap().text.contains("Cannot open"));
            let now = (
                s.project.clone(),
                s.path.clone(),
                s.dirty,
                s.selection.clone(),
                s.hidden_layers.clone(),
            );
            assert_eq!(now, snapshot, "{bad:?}");
        }
        // a failed DXF import does not touch the project either
        let before = s.project.clone();
        assert!(!s.import_dxf(&dir.join("nope.dxf")));
        assert_eq!(s.project, before);
    }

    #[test]
    fn library_path_none_never_touches_disk() {
        let mut s = AppState::new(None);
        assert!(s.library_error.is_some());
        assert!(s.messages.iter().any(|m| m.severity == Severity::Error));
        assert!(!s.library_can_save());
        // in-memory library still usable for operations
        s.library.add(tool(4)).unwrap();
        assert!(s.import_dxf(&bracket_path()));
        s.selection.insert(1);
        assert!(s.new_operation(NewKind::Drill, 4).is_some());
        assert_eq!(s.project.tools[0].id, 4);
        assert!(!s.save_library());
        s.start_new_library();
        assert!(!s.save_library(), "no path: nothing to write");
        assert!(!s.library_can_save());
    }

    #[test]
    fn new_operation_unknown_tool_and_project_copy_wins() {
        let mut s = loaded(&[1]);
        s.selection.insert(1);
        assert_eq!(s.new_operation(NewKind::Engrave, 99), None);
        assert!(s.project.operations.is_empty());
        s.new_operation(NewKind::Engrave, 1).unwrap();
        // library tool edited later: the project copy is offered and used
        s.library
            .update(Tool {
                feed: 1.0,
                ..tool(1)
            })
            .unwrap();
        assert_eq!(s.tool(1).unwrap().feed, 600.0);
        let b = s.new_operation(NewKind::Engrave, 1).unwrap();
        assert_eq!(
            s.project
                .operations
                .iter()
                .find(|o| o.id == b)
                .unwrap()
                .feed,
            600.0
        );
        assert_eq!(s.available_tools().len(), 1);
    }

    #[test]
    fn select_unknown_op_and_click_empty_space() {
        let mut s = loaded(&[]);
        s.selection = [1, 2].into();
        s.select_op(999);
        assert_eq!(s.selection, [1, 2].into());
        assert_eq!(s.selected_op, None);
        // shift-click on empty space keeps the selection
        s.click_select(Point::new(5000.0, 5000.0), 1.0, true);
        assert_eq!(s.selection, [1, 2].into());
        s.clear_selection();
        assert!(s.selection.is_empty());
    }

    #[test]
    fn m6_t13_tab_editing() {
        let mut s = AppState::new(None);
        let mut t = tool(1);
        t.diameter = 6.0;
        s.library.add(t).unwrap();
        let r = Contour::from_bulge_vertices(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]
                .map(|(x, y)| (Point::new(x, y), 0.0)),
            true,
        );
        s.project.items.push(Item {
            id: 1,
            layer: "0".into(),
            source: hc_project::Source::Drawn,
            geom: Geom::Contour(r),
        });
        s.project.next_id = 2;
        s.items_changed();
        s.selection.insert(1);
        let op = s.new_operation(NewKind::Contour, 1).unwrap();
        let mut o = s.project.operations[0].clone();
        o.tabs.enabled = true;
        o.depth = 6.0;
        o.depth_per_pass = 2.0;
        s.update_op(o);

        // no op selected: all no-ops
        s.selected_op = None;
        let before = s.project.clone();
        s.add_tab(1, Point::new(20.0, -1.0));
        s.move_tab(1, 0, Point::new(0.0, 20.0));
        s.delete_tab(1, 0);
        s.reset_tabs(op);
        assert_eq!(s.project, before);
        assert_eq!(s.tab_at(Point::new(37.5, 0.5), 1.0), None);
        s.select_op(op);

        let manual = |s: &AppState| s.project.operations[0].tabs.manual.clone();
        let mut last = {
            s.refresh_toolpaths();
            s.toolpaths.clone()
        };
        let mut step = |s: &mut AppState, f: &dyn Fn(&mut AppState)| {
            s.dirty = false;
            f(s);
            assert!(s.dirty);
            s.refresh_toolpaths();
            assert_ne!(s.toolpaths, last);
            assert!(s.toolpaths[0].1.is_ok());
            last = s.toolpaths.clone();
        };
        step(&mut s, &|s| s.add_tab(1, Point::new(20.0, -1.0)));
        let m = manual(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].item, 1);
        assert_eq!(m[0].positions.len(), 5);
        assert!((m[0].positions[4] - 20.0 / 300.0).abs() < 1e-9);
        assert!((m[0].positions[0] - 0.125).abs() < 1e-9);
        step(&mut s, &|s| s.move_tab(1, 4, Point::new(0.0, 20.0)));
        assert!((manual(&s)[0].positions[4] - 280.0 / 300.0).abs() < 1e-9);
        step(&mut s, &|s| s.delete_tab(1, 0));
        assert_eq!(manual(&s)[0].positions.len(), 4);
        step(&mut s, &|s| s.reset_tabs(op));
        assert!(manual(&s).is_empty());
        let auto = &s.tab_markers()[0].2;
        for (a, b) in auto.iter().zip([0.125, 0.375, 0.625, 0.875]) {
            assert!((a - b).abs() < 1e-9);
        }
        assert_eq!(s.tab_at(Point::new(37.5, 0.5), 1.0), Some((1, 0)));
        assert_eq!(s.tab_at(Point::new(37.5, 5.0), 1.0), None);
        // other items / disabled tabs: no-op
        let before = s.project.clone();
        s.add_tab(99, Point::new(0.0, 0.0));
        assert_eq!(s.project, before);
        // op selection change clears the selected tab
        s.selected_tab = Some((1, 0));
        s.new_project();
        assert_eq!(s.selected_tab, None);
    }

    /// R (100×50) as item 1 and a square (item 2) in one Contour op with tabs enabled, op selected.
    fn tab_state() -> (AppState, u64) {
        let mut s = AppState::new(None);
        let mut t = tool(1);
        t.diameter = 6.0;
        s.library.add(t).unwrap();
        for (id, (x0, w, h)) in [(1, (0.0, 100.0, 50.0)), (2, (200.0, 40.0, 40.0))] {
            let r = Contour::from_bulge_vertices(
                &[(x0, 0.0), (x0 + w, 0.0), (x0 + w, h), (x0, h)]
                    .map(|(x, y)| (Point::new(x, y), 0.0)),
                true,
            );
            s.project.items.push(Item {
                id,
                layer: "0".into(),
                source: hc_project::Source::Drawn,
                geom: Geom::Contour(r),
            });
        }
        s.project.next_id = 3;
        s.items_changed();
        s.selection = [1, 2].into_iter().collect();
        let op = s.new_operation(NewKind::Contour, 1).unwrap();
        let mut o = s.project.operations[0].clone();
        o.tabs.enabled = true;
        o.depth = 6.0;
        o.depth_per_pass = 2.0;
        s.update_op(o);
        s.select_op(op);
        (s, op)
    }

    #[test]
    fn m6_tab_edits_touch_only_their_item() {
        let (mut s, _) = tab_state();
        s.add_tab(1, Point::new(50.0, -2.0));
        let m = &s.project.operations[0].tabs.manual;
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].item, 1);
        assert!((m[0].positions[4] - 50.0 / 300.0).abs() < 1e-9);
        assert_eq!(s.selected_tab, Some((1, 4)));
        // item 2 still auto: 4 markers
        let markers = s.tab_markers();
        assert_eq!(markers.len(), 2);
        assert_eq!(markers.iter().find(|m| m.0 == 2).unwrap().2.len(), 4);
        assert_eq!(markers.iter().find(|m| m.0 == 1).unwrap().2.len(), 5);
    }

    #[test]
    fn m6_out_of_range_index_and_foreign_item() {
        let (mut s, op) = tab_state();
        let before = s.project.clone();
        // item not in the op / not existing: no-op
        s.add_tab(99, Point::new(0.0, 0.0));
        assert_eq!(s.project, before);
        // disabled tabs: no-op
        let mut o = s.project.operations[0].clone();
        o.tabs.enabled = false;
        s.update_op(o);
        let before = s.project.clone();
        s.add_tab(1, Point::new(10.0, 0.0));
        s.move_tab(1, 0, Point::new(10.0, 0.0));
        s.delete_tab(1, 0);
        s.reset_tabs(op);
        assert_eq!(s.project, before);
        assert!(s.tab_markers().is_empty());
        assert_eq!(s.tab_at(Point::new(37.5, 0.0), 1.0), None);
        // re-enabled: bad indices never panic or change positions
        let mut o = s.project.operations[0].clone();
        o.tabs.enabled = true;
        s.update_op(o);
        s.move_tab(1, 17, Point::new(10.0, 0.0));
        s.delete_tab(1, 17);
        let pos: Vec<f64> = s.project.operations[0]
            .tabs
            .manual
            .iter()
            .flat_map(|m| m.positions.clone())
            .collect();
        // either nothing was materialized or it holds exactly the auto positions
        assert!(
            pos.is_empty() || pos == [0.125, 0.375, 0.625, 0.875],
            "{pos:?}"
        );
    }

    #[test]
    fn m6_delete_every_tab_leaves_empty_manual_and_no_lifts() {
        let (mut s, op) = tab_state();
        for _ in 0..4 {
            s.delete_tab(1, 0);
        }
        let m = &s.project.operations[0].tabs.manual;
        assert_eq!(m.len(), 1);
        assert!(m[0].positions.is_empty());
        // the item keeps 0 tabs (an empty manual entry is not "auto")
        assert!(s
            .tab_markers()
            .iter()
            .find(|m| m.0 == 1)
            .unwrap()
            .2
            .is_empty());
        s.delete_tab(1, 0); // nothing left: no panic
        s.reset_tabs(op);
        assert!(s.project.operations[0].tabs.manual.is_empty());
        assert_eq!(
            s.tab_markers().iter().find(|m| m.0 == 1).unwrap().2.len(),
            4
        );
    }

    #[test]
    fn m6_remove_item_drops_manual_and_markers() {
        let (mut s, _) = tab_state();
        s.add_tab(2, Point::new(220.0, -1.0));
        assert_eq!(s.project.operations[0].tabs.manual.len(), 1);
        s.project.remove_item(2);
        s.items_changed();
        assert!(s.project.operations[0].tabs.manual.is_empty());
        assert_eq!(s.tab_markers().len(), 1);
    }

    #[test]
    fn m6_selected_tab_cleared_on_op_change() {
        let (mut s, op) = tab_state();
        s.selection = [1].into_iter().collect();
        let op2 = s.new_operation(NewKind::Contour, 1).unwrap();
        s.select_op(op);
        s.selected_tab = Some((1, 0));
        s.select_op(op); // same op: kept
        assert_eq!(s.selected_tab, Some((1, 0)));
        s.select_op(op2);
        assert_eq!(s.selected_tab, None);
        s.select_op(op);
        s.selected_tab = Some((1, 0));
        s.delete_op(op);
        assert_eq!(s.selected_tab, None);
    }

    #[test]
    fn m6_manual_tabs_survive_tool_change() {
        let (mut s, _) = tab_state();
        s.move_tab(1, 0, Point::new(50.0, 0.0));
        let before = s.project.operations[0].tabs.manual.clone();
        let mut t = tool(2);
        t.diameter = 10.0;
        s.library.add(t.clone()).unwrap();
        s.project.tools.push(t);
        let mut o = s.project.operations[0].clone();
        o.tool = 2;
        s.update_op(o);
        assert_eq!(s.project.operations[0].tabs.manual, before);
        s.refresh_toolpaths();
        assert!(s.toolpaths[0].1.is_ok());
    }

    #[test]
    fn m6_review_r5_out_of_range_index_is_noop() {
        let (mut s, _) = tab_state();
        s.dirty = false;
        let before = s.project.clone();
        s.move_tab(1, 4, Point::new(10.0, 0.0)); // auto has 4 tabs: 0..=3
        s.delete_tab(1, 4);
        s.move_tab(1, usize::MAX, Point::new(10.0, 0.0));
        s.delete_tab(3, 0); // not an item of the op
        assert_eq!(s.project, before);
        assert!(s.project.operations[0].tabs.manual.is_empty());
        assert!(!s.dirty);
        // in range still works
        s.move_tab(1, 3, Point::new(10.0, 0.0));
        assert!(s.dirty);
        assert_eq!(s.project.operations[0].tabs.manual[0].positions.len(), 4);
    }

    #[test]
    fn m6_review_r6_delete_shifts_selected_index() {
        let (mut s, _) = tab_state();
        s.selected_tab = Some((1, 2));
        s.delete_tab(1, 0);
        assert_eq!(s.selected_tab, Some((1, 1)));
        s.delete_tab(1, 2); // above the selection: unchanged
        assert_eq!(s.selected_tab, Some((1, 1)));
        s.delete_tab(2, 0); // other item: unchanged
        assert_eq!(s.selected_tab, Some((1, 1)));
        s.delete_tab(1, 1); // the selected one
        assert_eq!(s.selected_tab, None);
        assert_eq!(s.project.operations[0].tabs.manual[0].positions.len(), 1);
    }

    // ---------------------------------------------------------------- M7

    #[test]
    fn m7_t14_gcode_export() {
        let mut s = AppState::new(None);
        s.open_cli(&bracket_path());
        // failing enabled op: too large tool for an inside contour
        let hole = s
            .project
            .items
            .iter()
            .find(|i| matches!(&i.geom, Geom::Contour(c) if c.as_circle().is_some()))
            .unwrap()
            .id;
        let mut big = tool(9);
        big.diameter = 500.0;
        s.library.add(big).unwrap();
        s.selection = [hole].into_iter().collect();
        let op = s.new_operation(NewKind::Contour, 9).unwrap();
        let mut o = s
            .project
            .operations
            .iter()
            .find(|o| o.id == op)
            .unwrap()
            .clone();
        o.kind = OpKind::Contour {
            side: Side::Inside,
            direction: Direction::Climb,
        };
        s.update_op(o);
        let before = s.messages.len();
        assert!(s.gcode(&s.stem()).is_none());
        assert_eq!(s.messages.len(), before + 1);
        let m = s.messages.last().unwrap();
        assert_eq!(m.severity, Severity::Error);
        assert!(
            m.text
                .starts_with("Cannot export G-code: Cannot export: 1 error. First: Operation"),
            "{}",
            m.text
        );

        // two files written next to the chosen base name
        let dir = scratch("m7_t14");
        let files = vec![
            GcodeFile {
                suffix: "_1_T1".into(),
                text: "a\n".into(),
            },
            GcodeFile {
                suffix: "_2_T2".into(),
                text: "b\n".into(),
            },
        ];
        let dirty = s.dirty;
        let path = s.path.clone();
        assert!(s.write_gcode(&dir.join("out.ngc"), &files));
        assert_eq!(
            std::fs::read_to_string(dir.join("out_1_T1.ngc")).unwrap(),
            "a\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("out_2_T2.ngc")).unwrap(),
            "b\n"
        );
        assert_eq!(
            *s.messages.last().unwrap(),
            Message {
                severity: Severity::Info,
                text: "Exported 2 file(s): out_1_T1.ngc, out_2_T2.ngc".into()
            }
        );
        assert_eq!((s.dirty, &s.path), (dirty, &path));

        // missing directory
        assert!(!s.write_gcode(&dir.join("nope").join("out.ngc"), &files));
        let m = s.messages.last().unwrap();
        assert_eq!(m.severity, Severity::Error);
        assert!(
            m.text.starts_with("Cannot write out_1_T1.ngc: "),
            "{}",
            m.text
        );
        assert!(m.text.ends_with("Written: none"), "{}", m.text);

        // set_machine: dirty only on change
        let mut s = AppState::new(None);
        let m = s.project.machine.clone();
        s.set_machine(m.clone());
        assert!(!s.dirty);
        s.set_machine(Machine {
            tool_change_z: 50.0,
            ..m
        });
        assert!(s.dirty);
        assert_eq!(s.project.machine.tool_change_z, 50.0);
    }

    #[test]
    fn m7_gcode_ok_uses_project_stem() {
        let mut s = AppState::new(None);
        s.open_cli(&bracket_path());
        let hole = s
            .project
            .items
            .iter()
            .find(|i| matches!(&i.geom, Geom::Contour(c) if c.as_circle().is_some()))
            .unwrap()
            .id;
        s.library.add(tool(1)).unwrap();
        s.selection = [hole].into_iter().collect();
        s.new_operation(NewKind::Drill, 1).unwrap();
        let files = s.gcode(&s.stem()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].text.starts_with("(untitled - file 1 of 1)\n"));
        s.path = Some(PathBuf::from("/x/My part.hcam"));
        assert!(s.gcode(&s.stem()).unwrap()[0]
            .text
            .starts_with("(My part - file 1 of 1)\n"));
    }

    /// M11 R1: the header carries the export file name, also for an unsaved project.
    #[test]
    fn m11_header_uses_export_name() {
        let (mut s, _) = bracket_clean();
        assert_eq!(s.path, None);
        let files = s.gcode("bracket").unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0].text.starts_with("(bracket - file 1 of"));
    }

    #[test]
    fn m7_stale_files_warning() {
        let dir = scratch("m7_stale");
        let base = dir.join("out.ngc");
        let seg = |k: usize, t: u32| GcodeFile {
            suffix: format!("_{k}_T{t}"),
            text: format!("{k}\n"),
        };
        let mut s = AppState::new(None);
        assert!(s.write_gcode(&base, &[seg(1, 1), seg(2, 2), seg(3, 1)]));
        assert_eq!(s.messages.last().unwrap().severity, Severity::Info);
        // unrelated files and other stems are never listed
        std::fs::write(dir.join("other_1_T1.ngc"), "x").unwrap();
        std::fs::write(dir.join("out_notes.ngc"), "x").unwrap();
        std::fs::write(dir.join("out.txt"), "x").unwrap();
        // second export overwrites 1 and 2 (leaving .bak files) and leaves 3 behind
        assert!(s.write_gcode(&base, &[seg(1, 1), seg(2, 2)]));
        assert!(dir.join("out_1_T1.ngc.bak").exists());
        let n = s.messages.len();
        assert_eq!(s.messages[n - 2].severity, Severity::Info);
        assert_eq!(
            s.messages[n - 1],
            Message {
                severity: Severity::Warning,
                text: "Older files from a previous export are still in the folder: out_3_T1.ngc"
                    .into()
            }
        );
        assert!(dir.join("out_3_T1.ngc").exists()); // nothing deleted
                                                    // a single-file export next to segment files lists them all, and vice versa
        assert!(s.write_gcode(
            &base,
            &[GcodeFile {
                suffix: String::new(),
                text: "s\n".into()
            }]
        ));
        assert!(s
            .messages
            .last()
            .unwrap()
            .text
            .ends_with(": out_1_T1.ngc, out_2_T2.ngc, out_3_T1.ngc"));
        assert!(s.write_gcode(&base, &[seg(1, 1), seg(2, 2), seg(3, 1)]));
        assert!(s.messages.last().unwrap().text.ends_with("folder: out.ngc"));
        // no stale files → no warning
        let clean = scratch("m7_stale_clean");
        let n = s.messages.len();
        assert!(s.write_gcode(&clean.join("out.ngc"), &[seg(1, 1)]));
        assert_eq!(s.messages.len(), n + 1);
    }

    /// M9 test 9 (cross-check): sim blocks == exported files, tools in file order.
    #[test]
    fn m9_t9_blocks_match_export() {
        let (mut s, _) = bracket_clean();
        let drill = s.project.operations[2].id;
        s.move_op(drill, MoveDir::Up); // T1 Outside, T2 Drill, T1 Pocket
        s.refresh_toolpaths();
        let sim = s.sim.as_ref().unwrap();
        let files = hc_post::export(&s.project, "x").unwrap();
        assert_eq!(sim.blocks.len(), 3);
        assert_eq!(files.len(), 3);
        assert_eq!(
            sim.blocks.iter().map(|b| b.tool).collect::<Vec<_>>(),
            [1, 2, 1]
        );
        for (b, f) in sim.blocks.iter().zip(&files) {
            assert!(f.suffix.ends_with(&format!("_T{}", b.tool)), "{}", f.suffix);
        }
    }

    /// M9 test 15: timeline invariants on the M8 test 28 Bracket.
    #[test]
    fn m9_t15_monotonic() {
        let (mut s, _) = bracket_clean();
        s.refresh_toolpaths();
        let sim = s.sim.as_ref().unwrap();
        let starts: Vec<f64> = sim.blocks.iter().map(|b| b.t0).collect();
        let fin = |p: &hc_toolpath::Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
        let mut prev: Option<&hc_sim::Step> = None;
        for st in &sim.steps {
            assert!(st.t0 <= st.t1);
            assert!(fin(&st.from) && fin(&st.to) && st.t1.is_finite() && st.feed.is_finite());
            let p0 = st.point_at(st.t0);
            assert!(p0.xy().approx_eq(st.from.xy(), 1e-9) && (p0.z - st.from.z).abs() <= 1e-9);
            if let Some(pr) = prev {
                assert_eq!(st.t0, pr.t1);
                let block_start = starts.contains(&st.t0) && st.kind == hc_sim::StepKind::Dwell;
                if !block_start {
                    assert_eq!(st.from, pr.to);
                }
            }
            prev = Some(st);
        }
        assert!(
            sim.total() > 0.0 && sim.total() < 36000.0,
            "{}",
            sim.total()
        );
    }

    /// M9 test 16: playback state.
    #[test]
    fn m9_t16_app_state() {
        let (mut s, outside) = bracket_clean();
        s.refresh_toolpaths();
        let total = s.sim_total();
        assert!(!s.sim.as_ref().unwrap().steps.is_empty());
        s.advance(0.5);
        assert_eq!(s.sim_view.t, 0.0);
        s.toggle_play();
        // spec says advance(0.5) adds 5 s, which contradicts the 0.1 s clamp; the clamp wins
        s.advance(0.05);
        assert!((s.sim_view.t - 0.5).abs() < 1e-9);
        s.advance(10.0);
        assert!((s.sim_view.t - 1.5).abs() < 1e-9);
        s.sim_view.t = total - 0.1;
        s.advance(0.1);
        assert_eq!((s.sim_view.t, s.sim_view.playing), (total, false));
        s.toggle_play();
        assert_eq!((s.sim_view.t, s.sim_view.playing), (0.0, true));
        s.sim_view.t = total;
        edit_op(&mut s, outside, |o| o.depth = 2.0);
        s.refresh_toolpaths();
        assert!(s.sim_total() < total);
        assert_eq!(s.sim_view.t, s.sim_total());
    }

    /// M9 (verifier): the sim works in mm internally, so display units do not change the time.
    #[test]
    fn m9_inch_project_same_total() {
        let (mut s, _) = bracket_clean();
        s.refresh_toolpaths();
        let mm = s.sim.clone().unwrap();
        s.project.units = hc_project::Units::Inch;
        s.refresh_toolpaths();
        assert_eq!(s.sim.as_ref().unwrap(), &mm);
    }

    /// M9 (verifier): advance edge cases.
    #[test]
    fn m9_advance_edges() {
        let (mut s, _) = bracket_clean();
        s.refresh_toolpaths();
        let total = s.sim_total();
        s.toggle_play();
        s.advance(0.0);
        assert_eq!((s.sim_view.t, s.sim_view.playing), (0.0, true));
        s.sim_view.speed = 100.0;
        s.advance(f64::NAN); // f64::min ignores NaN → treated as the 0.1 s clamp
        assert!((s.sim_view.t - 10.0).abs() < 1e-9);
        s.sim_view.t = total;
        s.sim_view.playing = false;
        s.toggle_play(); // play at the end restarts
        assert_eq!(s.sim_view.t, 0.0);
        // empty project: total 0, play stops at once
        let mut e = AppState::new(None);
        e.refresh_toolpaths();
        assert_eq!(e.sim_total(), 0.0);
        e.toggle_play();
        e.advance(0.05);
        assert_eq!((e.sim_view.t, e.sim_view.playing), (0.0, false));
    }
}

#[cfg(test)]
mod m10_tests {
    use super::tests::{bracket_clean, bracket_path, edit_op, outer, scratch, tool};
    use super::*;
    use crate::snap::SnapKind;

    const T: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn only_item(s: &AppState) -> &Item {
        assert_eq!(s.project.items.len(), 1);
        &s.project.items[0]
    }

    fn rect_state() -> AppState {
        let mut s = AppState::new(None);
        s.add_shape(Shape::Rect {
            min: p(10.0, 20.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap();
        s
    }

    /// First G-code word value `k` of the first line starting with `prefix`.
    fn first_word(text: &str, prefix: &str, k: char) -> f64 {
        let line = text.lines().find(|l| l.starts_with(prefix)).unwrap();
        line.split(' ')
            .find_map(|w| w.strip_prefix(k))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn m10_11_set_origin() {
        let (mut s, _) = bracket_clean();
        s.refresh_toolpaths();
        let before = s.gcode(&s.stem()).unwrap();
        s.dirty = false;
        let edge = outer(&s);
        let idx = s.project.items.iter().position(|i| i.id == edge).unwrap();
        let corner = s
            .snap_cache
            .cands
            .iter()
            .find(|(i, _, _, k)| *i == idx && *k == SnapKind::Corner)
            .unwrap()
            .2;

        // no snap: nothing happens, the tool stays
        s.set_tool(CanvasTool::SetOrigin);
        s.canvas_click(p(corner.x + 3.0, corner.y), false);
        assert_eq!(s.project.stock.origin, WorkOrigin::DrawingOrigin);
        assert!(!s.dirty);
        assert_eq!(s.tool, CanvasTool::SetOrigin);

        s.canvas_click(corner, true);
        assert_eq!(s.project.stock.origin, WorkOrigin::Point(corner));
        assert!(s.dirty);
        assert_eq!(s.tool, CanvasTool::Select);
        s.refresh_toolpaths();
        assert_eq!(s.toolpaths_for.as_ref(), Some(&s.project));
        let after = s.gcode(&s.stem()).unwrap();
        let (x0, x1) = (
            first_word(&before[0].text, "G0 X", 'X'),
            first_word(&after[0].text, "G0 X", 'X'),
        );
        assert!((x0 - corner.x - x1).abs() < 1e-3, "{x0} {x1}");

        s.set_tool(CanvasTool::SetOrigin);
        s.escape();
        assert_eq!(s.tool, CanvasTool::Select);
        s.dirty = false;
        s.set_origin(WorkOrigin::Point(corner));
        assert!(!s.dirty);
    }

    #[test]
    fn m10_12_drawing() {
        let mut s = AppState::new(None);
        s.set_tool(CanvasTool::Line);
        s.canvas_click(p(0.0, 0.0), false);
        assert_eq!(s.pending, Some(p(0.0, 0.0)));
        assert!(s.project.items.is_empty());
        s.canvas_click(p(10.0, 0.0), false);
        let it = only_item(&s);
        let id = it.id;
        assert_eq!(it.layer, "Drawing");
        assert!(matches!(&it.geom, Geom::Contour(c) if !c.closed && (c.length() - 10.0).abs() < T));
        assert_eq!(s.selection, [id].into_iter().collect());
        assert!(s.dirty);
        assert_eq!(s.pending, None);
        assert_eq!(s.tool, CanvasTool::Line);

        // rectangle from two opposite corners
        let mut s = AppState::new(None);
        s.set_tool(CanvasTool::Rect);
        s.canvas_click(p(30.0, 20.0), false);
        s.canvas_click(p(10.0, 40.0), false);
        let b = s.item_bbox[0];
        assert!(b.min.approx_eq(p(10.0, 20.0), T) && b.max.approx_eq(p(30.0, 40.0), T));
        assert_eq!((s.draw.a, s.draw.w, s.draw.h), (p(10.0, 20.0), 20.0, 20.0));
        // the same point twice
        s.canvas_click(p(5.0, 5.0), false);
        s.canvas_click(p(5.0, 5.0), false);
        assert_eq!(s.project.items.len(), 1);
        assert_eq!(
            s.messages.last().unwrap(),
            &Message {
                severity: Severity::Warning,
                text: "Rectangle needs a width and a height".into()
            }
        );
        assert_eq!(s.pending, None);

        // circle: centre then rim
        let mut s = AppState::new(None);
        s.set_tool(CanvasTool::Circle);
        s.canvas_click(p(0.0, 0.0), false);
        s.canvas_click(p(3.0, 4.0), false);
        let Geom::Contour(c) = &only_item(&s).geom else {
            panic!()
        };
        assert!((c.as_circle().unwrap().radius - 5.0).abs() < T);

        // a snapped first click lands exactly on the candidate
        let mut s = rect_state();
        s.set_tool(CanvasTool::Line);
        let snap = s.snap_at(p(10.5, 20.3), 1.0).unwrap();
        assert_eq!(snap.p, p(10.0, 20.0));
        s.canvas_click(snap.p, true);
        s.canvas_click(p(0.0, 0.0), false);
        let Geom::Contour(c) = &s.project.items[1].geom else {
            panic!()
        };
        assert_eq!(c.start(), p(10.0, 20.0));

        // corner radius too large
        let mut s = AppState::new(None);
        let e = s
            .add_shape(Shape::Rect {
                min: p(0.0, 0.0),
                w: 10.0,
                h: 10.0,
                r: 6.0,
            })
            .unwrap_err();
        assert_eq!(e, "Corner radius must be at most 5.000 mm");
        assert!(s.project.items.is_empty() && !s.dirty);
        assert_eq!(
            s.add_shape(Shape::Line {
                a: p(1.0, 1.0),
                b: p(1.0, 1.0)
            }),
            Err("Line is too short".into())
        );
        assert_eq!(
            s.add_shape(Shape::Circle {
                c: p(1.0, 1.0),
                r: 0.0
            }),
            Err("Circle needs a diameter".into())
        );
        // escape: pending first, then the tool
        s.set_tool(CanvasTool::Rect);
        s.canvas_click(p(1.0, 1.0), false);
        s.escape();
        assert_eq!((s.pending, s.tool), (None, CanvasTool::Rect));
        s.escape();
        assert_eq!(s.tool, CanvasTool::Select);
    }

    #[test]
    fn m10_13_transforms_and_delete() {
        let mut s = rect_state();
        let b0 = s.selection_bbox();
        s.transform_selection(&Transform::translate(5.0, 0.0));
        let b = s.selection_bbox();
        assert!(b.min.approx_eq(p(b0.min.x + 5.0, b0.min.y), T));
        assert!(b.max.approx_eq(p(b0.max.x + 5.0, b0.max.y), T));

        let mut s = rect_state();
        let c = s.selection_center();
        s.transform_selection(&Transform::rotate(90f64.to_radians(), c));
        let b = s.selection_bbox();
        assert!(s.selection_center().approx_eq(c, 1e-9));
        assert!((b.width() - 10.0).abs() < T && (b.height() - 30.0).abs() < T);

        let mut s = rect_state();
        s.transform_selection(&Transform::scale(2.0, c));
        let b = s.selection_bbox();
        assert!(s.selection_center().approx_eq(c, 1e-9));
        assert!((b.width() - 60.0).abs() < T && (b.height() - 20.0).abs() < T);

        let mut s = rect_state();
        let orig = s.project.items.clone();
        s.transform_selection(&Transform::mirror_y(c.x));
        s.transform_selection(&Transform::mirror_y(c.x));
        let (Geom::Contour(a), Geom::Contour(b)) = (&orig[0].geom, &s.project.items[0].geom) else {
            panic!()
        };
        assert_eq!(a.segments.len(), b.segments.len());
        for (x, y) in a.segments.iter().zip(&b.segments) {
            assert!(x.start().approx_eq(y.start(), T) && x.end().approx_eq(y.end(), T));
        }

        // empty selection: no change, not dirty
        let mut s = rect_state();
        s.clear_selection();
        s.dirty = false;
        let before = s.project.clone();
        s.transform_selection(&Transform::translate(1.0, 1.0));
        s.delete_selection();
        assert_eq!(s.project, before);
        assert!(!s.dirty);

        // delete an item used by an op with manual tabs
        let (mut s, outside) = bracket_clean();
        let edge = outer(&s);
        edit_op(&mut s, outside, |o| {
            o.tabs.manual = vec![ManualTabs {
                item: edge,
                positions: vec![0.5],
            }]
        });
        s.dirty = false;
        s.selection = [edge].into_iter().collect();
        s.delete_selection();
        assert!(s.item(edge).is_none());
        let op = s
            .project
            .operations
            .iter()
            .find(|o| o.id == outside)
            .unwrap();
        assert!(op.items.is_empty() && op.tabs.manual.is_empty());
        assert!(s.selection.is_empty() && s.dirty);
        assert_eq!(s.item_bbox.len(), s.project.items.len());
        assert!(s
            .snap_cache
            .cands
            .iter()
            .all(|(i, j, _, _)| *i < s.project.items.len() && *j < s.project.items.len()));
    }

    #[test]
    fn m10_14_stock() {
        let mut s = AppState::new(None);
        let mut st = s.project.stock.clone();
        s.set_stock(st.clone());
        assert!(!s.dirty);
        st.thickness = 12.0;
        s.set_stock(st);
        assert!(s.dirty);

        let mut s = AppState::new(None);
        s.new_project();
        s.set_origin(WorkOrigin::StockCenter);
        assert!(s.import_dxf(&bracket_path()));
        let b = s.items_bbox();
        let st = &s.project.stock;
        assert!((st.min_x - b.min.x).abs() < T && (st.width - b.width()).abs() < T);
        assert_eq!(st.origin, WorkOrigin::StockCenter);
    }

    #[test]
    fn m10_16_undo_basics() {
        let mut s = AppState::new(None);
        let p0 = s.project.clone();
        s.add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 10.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap();
        s.commit_undo(true);
        let p1 = s.project.clone();
        s.transform_selection(&Transform::translate(5.0, 0.0));
        s.commit_undo(true);
        let p2 = s.project.clone();
        s.delete_selection();
        s.commit_undo(true);
        let p3 = s.project.clone();
        for want in [&p2, &p1, &p0] {
            assert!(s.undo());
            assert_eq!(&s.project, want);
        }
        assert!(!s.undo());
        for want in [&p1, &p2, &p3] {
            assert!(s.redo());
            assert_eq!(&s.project, want);
        }
        assert!(!s.redo());
        s.undo();
        assert!(s.can_redo());
        s.set_origin(WorkOrigin::StockCenter);
        s.commit_undo(true);
        assert!(!s.can_redo());

        // each kind of edit is undoable
        let (mut s, outside) = bracket_clean();
        s.commit_undo(true);
        let edits: [fn(&mut AppState, u64); 5] = [
            |s, _| s.set_origin(WorkOrigin::StockBottomLeft),
            |s, _| {
                let mut st = s.project.stock.clone();
                st.thickness = 3.0;
                s.set_stock(st)
            },
            |s, outside| edit_op(s, outside, |o| o.depth = 4.0),
            |s, outside| {
                s.select_op(outside);
                let (item, _, pos) = s.tab_markers()[0].clone();
                let c = match &s.item(item).unwrap().geom {
                    Geom::Contour(c) => c.clone(),
                    _ => panic!(),
                };
                let at = c.point_at((pos[0] + 0.01) * c.length());
                s.move_tab(item, 0, at);
            },
            |s, _| assert!(s.import_dxf(&bracket_path())),
        ];
        for (k, edit) in edits.iter().enumerate() {
            let before = s.project.clone();
            edit(&mut s, outside);
            assert_ne!(s.project, before, "edit {k}");
            s.commit_undo(true);
            assert!(s.undo());
            assert_eq!(s.project, before, "edit {k}");
        }
    }

    #[test]
    fn m10_17_coalescing() {
        let (mut s, outside) = bracket_clean();
        s.commit_undo(true);
        let n = s.undo.len();
        let before = s.project.clone();
        for k in 0..10 {
            edit_op(&mut s, outside, |o| o.depth = 0.5 + k as f64);
            s.commit_undo(false);
        }
        s.commit_undo(true);
        assert_eq!(s.undo.len(), n + 1);
        s.commit_undo(true);
        assert_eq!(s.undo.len(), n + 1);
        assert!(s.undo());
        assert_eq!(s.project, before);
    }

    #[test]
    fn m10_18_depth() {
        let mut s = AppState::new(None);
        let mut states = vec![s.project.clone()];
        for k in 0..105 {
            let mut st = s.project.stock.clone();
            st.thickness = 1.0 + k as f64;
            s.set_stock(st);
            s.commit_undo(true);
            states.push(s.project.clone());
        }
        assert_eq!(s.undo.len(), UNDO_DEPTH);
        for k in 0..100 {
            assert!(s.undo(), "{k}");
            assert_eq!(s.project, states[104 - k]);
        }
        assert!(!s.undo());
        // the oldest 5 states (0..5) are gone
        assert_eq!(s.project, states[5]);
    }

    #[test]
    fn m10_19_dirty() {
        let dir = scratch("m10_19");
        let path = dir.join("a.hcam");
        let (mut s, _) = bracket_clean();
        assert!(s.save_as(&path));
        let mut s2 = AppState::new(None);
        assert!(s2.open(&path));
        let mut s = s2;
        assert!(!s.dirty);
        s.set_origin(WorkOrigin::StockCenter);
        s.commit_undo(true);
        assert!(s.dirty);
        assert!(s.undo());
        assert!(!s.dirty);
        assert!(s.redo());
        assert!(s.dirty);
        assert!(s.save_as(&path));
        assert!(!s.dirty);
        assert!(s.undo());
        assert!(s.dirty);

        // an uncommitted change is committed first, so it can be redone
        let mut s = AppState::new(None);
        s.set_origin(WorkOrigin::StockCenter);
        let changed = s.project.clone();
        assert!(s.can_undo());
        assert!(s.undo());
        assert_eq!(s.project.stock.origin, WorkOrigin::DrawingOrigin);
        assert!(s.redo());
        assert_eq!(s.project, changed);
    }

    #[test]
    fn m10_20_reset_and_scope() {
        let mut s = rect_state();
        s.commit_undo(true);
        assert!(s.can_undo());
        s.new_project();
        assert!(!s.can_undo() && !s.can_redo());
        let dir = scratch("m10_20");
        let path = dir.join("a.hcam");
        let mut s = rect_state();
        assert!(s.save_as(&path));
        s.commit_undo(true);
        assert!(s.open(&path));
        assert!(!s.can_undo());

        // selection, hidden layers and the library are not in the history
        let mut s = AppState::new(Some(dir.join("tools.json")));
        assert!(s.library_add(tool(3)));
        s.commit_undo(true);
        assert!(!s.can_undo());
        assert!(!s.undo());
        assert_eq!(s.library.tools.len(), 1);
        let ids = s
            .add_shape(Shape::Circle {
                c: p(0.0, 0.0),
                r: 2.0,
            })
            .unwrap();
        s.commit_undo(true);
        s.selection = ids.iter().copied().collect();
        s.set_layer_visible("Drawing", false);
        s.selection = ids.iter().copied().collect();
        s.delete_selection();
        s.commit_undo(true);
        s.selected_tab = Some((ids[0], 0));
        assert!(s.undo());
        assert_eq!(s.project.items.len(), 1);
        assert!(s.selection.is_empty());
        assert!(s.hidden_layers.contains("Drawing"));
        assert_eq!(s.library.tools.len(), 1);
        assert_eq!(s.item_bbox.len(), 1);
        assert_eq!(s.snap_cache, snap::build_cache(&s.project.items));
        assert_eq!(s.selected_tab, None);
        s.selection = ids.iter().copied().collect();
        assert!(s.undo()); // back to empty: the selected item no longer exists
        assert!(s.selection.is_empty());
    }

    /// M10b test 29.
    #[test]
    fn m10_29_text() {
        let dir = scratch("m10_29");
        let mut s = AppState::new(Some(dir.join("tools.json")));
        // Light strokes of a 10 mm text are about 1 mm wide: a small end mill
        assert!(s.library_add(Tool {
            diameter: 0.5,
            ..tool(1)
        }));
        s.set_tool(CanvasTool::Text);
        assert_eq!(
            s.tool.hint(false),
            Some("Text: click the start of the baseline")
        );
        s.draw.text = "HO".into();
        s.draw.font = hc_text::Font::Outline;
        s.draw.height = 10.0;
        s.canvas_click(p(0.0, 0.0), false);
        assert_eq!(s.project.items.len(), 3);
        assert!(s
            .project
            .items
            .iter()
            .all(|i| i.layer == "Drawing" && matches!(&i.geom, Geom::Contour(c) if c.closed)));
        let ids: Vec<u64> = s.project.items.iter().map(|i| i.id).collect();
        assert_eq!(s.selection, ids.iter().copied().collect());
        assert!(s.dirty);
        assert_eq!(s.tool, CanvasTool::Text);
        assert_eq!(s.draw.a, p(0.0, 0.0));
        assert_eq!(s.item_bbox.len(), 3);
        s.new_operation(NewKind::Pocket, 1).unwrap();
        s.refresh_toolpaths();
        assert!(s.toolpaths[0].1.is_ok(), "{:?}", s.toolpaths[0].1);
        // M10b ruling 2: the flattened curves must not raise a false PocketUncut
        assert!(
            !s.issues
                .iter()
                .any(|i| i.rule == hc_toolpath::RuleId::PocketUncut),
            "{:?}",
            s.issues
        );

        // Hershey: open strokes, Engrave fine, Pocket is an OpenContour error
        let mut s = AppState::new(Some(dir.join("tools.json")));
        s.set_tool(CanvasTool::Text);
        s.draw.text = "HI".into();
        s.canvas_click(p(5.0, 5.0), false);
        assert_eq!(s.project.items.len(), 4);
        assert!(s
            .project
            .items
            .iter()
            .all(|i| matches!(&i.geom, Geom::Contour(c) if !c.closed)));
        let engrave = s.new_operation(NewKind::Engrave, 1).unwrap();
        let pocket = s.new_operation(NewKind::Pocket, 1).unwrap();
        s.refresh_toolpaths();
        let result = |id| &s.toolpaths.iter().find(|t| t.0 == id).unwrap().1;
        assert!(result(engrave).is_ok());
        assert!(matches!(result(pocket), Err(ToolpathError::OpenContour(_))));
        assert!(s
            .issues
            .iter()
            .any(|i| i.rule == hc_toolpath::RuleId::OpenContour
                && i.severity == hc_toolpath::Severity::Error
                && i.op == Some(pocket)));

        // a missing character is reported, the rest is still added
        let mut s = AppState::new(None);
        s.set_tool(CanvasTool::Text);
        s.draw.text = "Hé".into();
        s.canvas_click(p(0.0, 0.0), true);
        assert_eq!(s.project.items.len(), 3);
        assert_eq!(
            s.messages.last().unwrap(),
            &Message {
                severity: Severity::Warning,
                text: "Not in the font: é".into()
            }
        );
        // the UI font cannot show it either: code point instead
        s.draw.font = hc_text::Font::Outline;
        s.draw.text = "中\tA".into();
        s.canvas_click(p(0.0, 20.0), false);
        assert_eq!(
            s.messages.last().unwrap().text,
            "Not in the font: U+4E2D, U+0009"
        );
    }

    #[test]
    fn m10_text_errors() {
        let mut s = AppState::new(None);
        let text = |h: f64, t: &str| Shape::Text {
            at: p(0.0, 0.0),
            text: t.into(),
            font: hc_text::Font::Hershey,
            height: h,
        };
        for h in [0.0, -1.0, f64::NAN] {
            assert_eq!(s.add_shape(text(h, "H")), Err("Text needs a height".into()));
        }
        // nothing to draw: no item, not dirty
        assert_eq!(s.add_shape(text(10.0, "  ")), Ok(vec![]));
        assert!(s.project.items.is_empty());
        assert!(!s.dirty);
        // the form default is engraving text
        let f = DrawForm::default();
        assert_eq!(
            (f.text.as_str(), f.font, f.height),
            ("HobbyCAM", hc_text::Font::Hershey, 10.0)
        );
    }
}

#[cfg(test)]
mod m13_tests {
    use super::tests::{bracket_clean, circles, outer};
    use super::*;
    use hc_project::Geom;

    fn on(s: &AppState, id: u64) -> Point {
        match &s.item(id).unwrap().geom {
            Geom::Contour(c) => c.point_at(0.0),
            Geom::Point(p) => *p,
        }
    }

    /// A Ø15.875 hole; the bracket has an identical duplicate on another layer, so take the one a click picks.
    fn hole(s: &AppState) -> u64 {
        let c = circles(s, 15.875)[0];
        hit::pick(&s.project.items, |i| s.is_visible(i), on(s, c), 0.5).unwrap()
    }

    #[test]
    fn m13_click_item_replaces_selected_op() {
        let (mut s, _) = bracket_clean();
        let pocket = s.project.operations[1].id;
        s.select_op(pocket);
        let hole = hole(&s);
        s.click_select(on(&s, hole), 0.5, false);
        assert_eq!(s.selected_op, None);
        assert_eq!(s.selection, BTreeSet::from([hole]));
        assert_eq!(s.selected_tab, None);
        // click on empty canvas
        s.select_op(pocket);
        s.click_select(Point::new(-1000.0, -1000.0), 0.5, false);
        assert!(s.selection.is_empty());
        assert_eq!(s.selected_op, None);
    }

    #[test]
    fn m13_shift_click_keeps_op_for_use_selection() {
        let (mut s, _) = bracket_clean();
        let pocket = s.project.operations[1].id;
        s.select_op(pocket);
        assert_eq!(s.selection.len(), 2);
        let hole = hole(&s);
        s.click_select(on(&s, hole), 0.5, true);
        assert_eq!(s.selected_op, Some(pocket));
        assert_eq!(s.selection.len(), 3);
        s.use_selection(pocket);
        assert_eq!(s.project.operations[1].items.len(), 3);
        s.click_select(on(&s, hole), 0.5, true);
        assert_eq!(s.selection.len(), 2);
        assert_eq!(s.selected_op, Some(pocket));
    }

    #[test]
    fn m13_click_on_tab_contour_keeps_op() {
        let (mut s, outside) = bracket_clean();
        let edge = outer(&s);
        s.select_op(outside);
        s.click_select(on(&s, edge), 0.5, false);
        assert_eq!(s.selected_op, Some(outside));
        assert_eq!(s.selection, BTreeSet::from([edge]));
        assert!(!s.tab_markers().is_empty());
        let hole = hole(&s);
        s.click_select(on(&s, hole), 0.5, false);
        assert_eq!(s.selected_op, None);
        assert_eq!(s.selection, BTreeSet::from([hole]));
        assert!(s.tab_markers().is_empty());
    }

    #[test]
    fn m13_box_select_plain_clears_op_shift_keeps_it() {
        let (mut s, outside) = bracket_clean();
        s.select_op(outside);
        let (a, b) = (Point::new(-1000.0, -1000.0), Point::new(1000.0, 1000.0));
        s.box_select(a, b, true, 0.5);
        assert_eq!(s.selected_op, Some(outside));
        s.box_select(a, b, false, 0.5);
        assert_eq!(s.selected_op, None);
        assert_eq!(s.selection.len(), s.project.items.len());
    }

    #[test]
    fn m13_drawn_shape_becomes_current() {
        let (mut s, outside) = bracket_clean();
        s.select_op(outside);
        let ids = s
            .add_shape(Shape::Circle {
                c: Point::new(0.0, 0.0),
                r: 5.0,
            })
            .unwrap();
        assert_eq!(s.selected_op, None);
        assert_eq!(s.selection, ids.into_iter().collect());
    }

    #[test]
    fn m13_escape_clears_op() {
        let (mut s, outside) = bracket_clean();
        s.select_op(outside);
        s.escape();
        assert_eq!(s.selected_op, None);
        assert!(s.selection.is_empty());
        s.select_op(outside);
        s.tool = CanvasTool::Line;
        s.escape();
        assert_eq!(s.selected_op, Some(outside));
        s.escape();
        assert_eq!(s.selected_op, None);
    }

    #[test]
    fn m13_edge_shift_empty_delete_undo_issue() {
        let (mut s, outside) = bracket_clean();
        // Shift+click on empty canvas changes nothing
        s.select_op(outside);
        let before = s.selection.clone();
        s.click_select(Point::new(-1000.0, -1000.0), 0.5, true);
        assert_eq!(s.selected_op, Some(outside));
        assert_eq!(s.selection, before);
        // plain click on empty canvas while a tab op is current clears it and the markers
        s.click_select(Point::new(-1000.0, -1000.0), 0.5, false);
        assert_eq!(s.selected_op, None);
        assert!(s.tab_markers().is_empty());
        // delete the selected op
        s.select_op(outside);
        s.delete_op(outside);
        assert_eq!(s.selected_op, None);
        assert_eq!(s.selected_tab, None);
        // undo restores the op without making it current; redo keeps nothing stale
        if s.undo() {
            assert!(s
                .selected_op
                .is_none_or(|id| s.project.operations.iter().any(|o| o.id == id)));
            s.redo();
            assert!(s
                .selected_op
                .is_none_or(|id| s.project.operations.iter().any(|o| o.id == id)));
        }
        // selecting an op again, then a plain click, then undo: no stale op id
        let first = s.project.operations[0].id;
        s.select_op(first);
        s.click_select(Point::new(-1000.0, -1000.0), 0.5, false);
        s.undo();
        s.redo();
        assert!(s
            .selected_op
            .is_none_or(|id| s.project.operations.iter().any(|o| o.id == id)));
    }
}
