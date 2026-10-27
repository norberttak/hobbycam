//! Application state and every project edit. No egui UI code here.

use crate::hit;
use crate::view::View;
use hc_dxf::{ImportOptions, ImportWarning};
use hc_geom::{BBox, Contour, Point};
use hc_post::GcodeFile;
use hc_project::{
    Direction, Geom, Item, Machine, ManualTabs, OpKind, Operation, Project, Side, Tool,
    ToolLibrary, Units,
};
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
        }
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

    fn items_changed(&mut self) {
        self.item_bbox = self.project.items.iter().map(hit::item_bbox).collect();
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
        let untouched =
            self.project.stock == Project::new().stock && self.project.operations.is_empty();
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

    /// Project file stem, or "untitled".
    pub fn stem(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string())
    }

    /// The G-code files of the project, or None with an Error message.
    pub fn gcode(&mut self) -> Option<Vec<GcodeFile>> {
        match hc_post::export(&self.project, &self.stem()) {
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
            if let Some(id) = hit {
                if !self.selection.remove(&id) {
                    self.selection.insert(id);
                }
            }
        } else {
            self.selection = hit.into_iter().collect();
        }
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

    fn op_on(s: &mut AppState, items: Vec<u64>, kind: NewKind, tool: u32) -> u64 {
        s.selection = items.into_iter().collect();
        s.new_operation(kind, tool).unwrap()
    }

    fn edit_op(s: &mut AppState, id: u64, f: impl FnOnce(&mut Operation)) {
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
        assert!(s.library_error.is_some()); // reading through a file path fails too
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
    use super::tests::{bracket_path, scratch, tool};
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
        assert!(s.gcode().is_none());
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
        let files = s.gcode().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].text.starts_with("(untitled - file 1 of 1)\n"));
        s.path = Some(PathBuf::from("/x/My part.hcam"));
        assert!(s.gcode().unwrap()[0]
            .text
            .starts_with("(My part - file 1 of 1)\n"));
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
}
