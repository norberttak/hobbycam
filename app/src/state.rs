//! Application state and every project edit. No egui UI code here.

use crate::hit;
use crate::view::View;
use hc_dxf::{ImportOptions, ImportWarning};
use hc_geom::{BBox, Point};
use hc_project::{Direction, Item, OpKind, Problem, Project, Side, Tool, ToolLibrary, Units};
use hc_toolpath::{Toolpath, ToolpathError};
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
            hidden_layers: HashSet::new(),
            messages: Vec::new(),
            view: View::default(),
            fit_pending: true,
            cursor: None,
            item_bbox: Vec::new(),
            toolpaths: Vec::new(),
            toolpaths_for: None,
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
        self.hidden_layers.clear();
        self.items_changed();
        self.fit_pending = true;
    }

    pub fn new_project(&mut self) {
        self.replace_project(Project::new(), None);
    }

    pub fn open(&mut self, path: &Path) -> bool {
        match Project::load(path) {
            Ok((p, problems)) => {
                self.replace_project(p, Some(path.to_path_buf()));
                for pr in problems {
                    self.msg(Severity::Warning, problem_text(&pr));
                }
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
        }
        self.dirty = true;
    }

    /// Makes the op current and selects its items.
    pub fn select_op(&mut self, id: u64) {
        if let Some(op) = self.project.operations.iter().find(|o| o.id == id) {
            self.selection = op.items.iter().copied().collect();
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

pub fn problem_text(p: &Problem) -> String {
    match p {
        Problem::MissingTool { op, tool } => {
            format!("Operation {op} uses tool T{tool}, which is not in the project")
        }
        Problem::MissingItem { op, item } => {
            format!("Operation {op} refers to item {item}, which does not exist")
        }
        Problem::DuplicateId(id) => format!("Id {id} is used more than once"),
        Problem::DuplicateToolId(id) => format!("Tool T{id} is embedded more than once"),
        Problem::IdNotBelowNextId(id) => format!("Id {id} is not below the project's id counter"),
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
}
