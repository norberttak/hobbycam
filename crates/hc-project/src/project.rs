use crate::io::{atomic_write, from_versioned_json, to_checked_json, FORMAT_VERSION};
use crate::{ProjectError, Tool, Units};
use hc_dxf::DxfImport;
use hc_geom::{Contour, Point};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// XY zero of the G-code; Z0 is always the stock top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOrigin {
    DrawingOrigin,
    StockBottomLeft,
    StockCenter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stock {
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    pub origin: WorkOrigin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub safe_z: f64,
    pub clearance_z: f64,
    pub post: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerInfo {
    pub name: String,
    pub color: i16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Imported { file: String },
    Drawn,
}

/// A new variant here is a format version bump (old apps must refuse the file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geom {
    Contour(Contour),
    Point(Point),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub layer: String,
    pub source: Source,
    pub geom: Geom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Inside,
    Outside,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Climb,
    Conventional,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpKind {
    Engrave,
    Contour {
        side: Side,
        direction: Direction,
    },
    Pocket {
        stepover_pct: f64,
    },
    /// peck: depth per peck in mm, None = single plunge.
    Drill {
        peck: Option<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TabPlacement {
    Count { count: u32 },
    Spacing { distance: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    pub enabled: bool,
    pub placement: TabPlacement,
    pub width: f64,
    pub height: f64,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs {
            enabled: false,
            placement: TabPlacement::Count { count: 4 },
            width: 5.0,
            height: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub tool: u32,
    pub items: Vec<u64>,
    pub kind: OpKind,
    /// Final depth, positive = below stock top.
    pub depth: f64,
    pub depth_per_pass: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub rpm: u32,
    pub tabs: Tabs,
}

/// Non-fatal consistency problem found by `Project::validate`.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    MissingTool { op: u64, tool: u32 },
    MissingItem { op: u64, item: u64 },
    DuplicateId(u64),
    DuplicateToolId(u32),
    IdNotBelowNextId(u64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub units: Units,
    pub stock: Stock,
    pub machine: Machine,
    pub layers: Vec<LayerInfo>,
    pub items: Vec<Item>,
    /// Embedded tool copies; on load they win over the library.
    pub tools: Vec<Tool>,
    /// Execution order.
    pub operations: Vec<Operation>,
    /// Shared id counter for items and operations; ids are never reused.
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Project::new()
    }
}

impl Project {
    pub fn new() -> Project {
        Project {
            format_version: FORMAT_VERSION,
            units: Units::Mm,
            stock: Stock {
                min_x: 0.0,
                min_y: 0.0,
                width: 100.0,
                height: 100.0,
                thickness: 10.0,
                origin: WorkOrigin::DrawingOrigin,
            },
            machine: Machine {
                safe_z: 5.0,
                clearance_z: 2.0,
                post: "linuxcnc".to_string(),
            },
            layers: Vec::new(),
            items: Vec::new(),
            tools: Vec::new(),
            operations: Vec::new(),
            next_id: 1,
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// One item per contour and per point; returns the new ids.
    pub fn add_dxf(&mut self, imp: &DxfImport, file: &str) -> Vec<u64> {
        let mut ids = Vec::new();
        for layer in &imp.layers {
            if !self.layers.iter().any(|l| l.name == layer.name) {
                self.layers.push(LayerInfo {
                    name: layer.name.clone(),
                    color: layer.color,
                });
            }
            let geoms = layer
                .contours
                .iter()
                .map(|c| Geom::Contour(c.clone()))
                .chain(layer.points.iter().map(|&p| Geom::Point(p)));
            for geom in geoms {
                let id = self.take_id();
                self.items.push(Item {
                    id,
                    layer: layer.name.clone(),
                    source: Source::Imported {
                        file: file.to_string(),
                    },
                    geom,
                });
                ids.push(id);
            }
        }
        ids
    }

    /// New operation with cutting data from `tool`; `tool` is embedded, replacing an older copy with the same id.
    /// Existing operations keep their own feed/rpm/depth values.
    pub fn add_operation(&mut self, name: &str, kind: OpKind, tool: &Tool, items: Vec<u64>) -> u64 {
        // one physical T<n>: the passed (current) tool replaces an embedded copy
        match self.tools.iter_mut().find(|t| t.id == tool.id) {
            Some(t) => *t = tool.clone(),
            None => self.tools.push(tool.clone()),
        }
        let kind = match kind {
            OpKind::Pocket { .. } => OpKind::Pocket {
                stepover_pct: tool.stepover_pct,
            },
            k => k,
        };
        let id = self.take_id();
        self.operations.push(Operation {
            id,
            name: name.to_string(),
            enabled: true,
            tool: tool.id,
            items,
            kind,
            depth: tool.depth_per_pass,
            depth_per_pass: tool.depth_per_pass,
            feed: tool.feed,
            plunge_feed: tool.plunge_feed,
            rpm: tool.rpm,
            tabs: Tabs::default(),
        });
        id
    }

    /// Removes the item and every reference to it.
    pub fn remove_item(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
        for op in &mut self.operations {
            op.items.retain(|&i| i != id);
        }
    }

    pub fn remove_operation(&mut self, id: u64) {
        self.operations.retain(|o| o.id != id);
    }

    /// Embedded tools that no operation uses.
    pub fn unused_tools(&self) -> Vec<u32> {
        self.tools
            .iter()
            .map(|t| t.id)
            .filter(|id| !self.operations.iter().any(|o| o.tool == *id))
            .collect()
    }

    pub fn prune_tools(&mut self) {
        let unused = self.unused_tools();
        self.tools.retain(|t| !unused.contains(&t.id));
    }

    /// Dangling references and id clashes. Never fatal.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let item_ids: HashSet<u64> = self.items.iter().map(|i| i.id).collect();
        for op in &self.operations {
            if !self.tools.iter().any(|t| t.id == op.tool) {
                problems.push(Problem::MissingTool {
                    op: op.id,
                    tool: op.tool,
                });
            }
            for &item in &op.items {
                if !item_ids.contains(&item) {
                    problems.push(Problem::MissingItem { op: op.id, item });
                }
            }
        }
        let mut seen = HashSet::new();
        let mut reported = HashSet::new();
        for id in self
            .items
            .iter()
            .map(|i| i.id)
            .chain(self.operations.iter().map(|o| o.id))
        {
            // at most one id problem per id: IdNotBelowNextId on first sight, else DuplicateId
            if seen.insert(id) {
                if id >= self.next_id {
                    reported.insert(id);
                    problems.push(Problem::IdNotBelowNextId(id));
                }
            } else if reported.insert(id) {
                problems.push(Problem::DuplicateId(id));
            }
        }
        let mut seen_tools = HashSet::new();
        for t in &self.tools {
            if !seen_tools.insert(t.id) {
                problems.push(Problem::DuplicateToolId(t.id));
            }
        }
        problems
    }

    /// Parse + version check + migrate + validate, without I/O.
    pub fn from_json(text: &str) -> Result<(Project, Vec<Problem>), ProjectError> {
        let mut p: Project = from_versioned_json(text)?;
        p.format_version = FORMAT_VERSION;
        let problems = p.validate();
        Ok((p, problems))
    }

    pub fn load(path: &Path) -> Result<(Project, Vec<Problem>), ProjectError> {
        Project::from_json(&std::fs::read_to_string(path)?)
    }

    /// Pretty JSON + trailing newline. Err(NonFinite) when a value is NaN/inf.
    pub fn to_json(&self) -> Result<String, ProjectError> {
        to_checked_json(self)
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        atomic_write(path, self.to_json()?.as_bytes())
    }
}
