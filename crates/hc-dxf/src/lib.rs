//! HobbyCAM DXF import: ASCII DXF (R12 … 2018) → hc-geom contours and points in mm, grouped by layer.

mod entities;
mod flatten;
mod scan;

use hc_geom::{BBox, Contour, Point};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    /// Max chord deviation when curves are turned into lines (mm).
    pub flatten_tol_mm: f64,
    /// Endpoint tolerance for chaining loose segments (mm).
    pub chain_tol_mm: f64,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            flatten_tol_mm: 0.01,
            chain_tol_mm: hc_geom::DEFAULT_CHAIN_TOL,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// ACI colour from the LAYER table (abs); 7 if the layer is not in the table.
    pub color: i16,
    /// mm
    pub contours: Vec<Contour>,
    /// POINT entities, mm (drill candidates).
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DxfImport {
    /// Only layers that received geometry/points, in first-appearance order.
    pub layers: Vec<Layer>,
    /// mm per drawing unit.
    pub unit_scale: f64,
    /// DXF type name → count of entities that were not imported.
    pub ignored: BTreeMap<String, usize>,
    pub warnings: Vec<ImportWarning>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportWarning {
    UnitsAssumedMm { insunits: i16 },
    UnsupportedExtrusion { entity: String },
    PaperSpaceSkipped { count: usize },
    MissingBlock { name: String },
    InsertArrayFirstOnly { block: String },
    SplineFitPointsOnly { layer: String },
    InsertNestingTooDeep { block: String },
}

#[derive(Debug)]
pub enum DxfError {
    Io(std::io::Error),
    Parse(String),
}

impl fmt::Display for DxfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DxfError::Io(e) => write!(f, "cannot read DXF file: {e}"),
            DxfError::Parse(m) => write!(f, "invalid DXF: {m}"),
        }
    }
}

impl std::error::Error for DxfError {}

impl From<dxf::DxfError> for DxfError {
    fn from(e: dxf::DxfError) -> Self {
        match e {
            dxf::DxfError::IoError(io) => DxfError::Io(io),
            other => DxfError::Parse(other.to_string()),
        }
    }
}

impl DxfImport {
    /// Bounding box of all contours and points of all layers.
    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        for l in &self.layers {
            for c in &l.contours {
                b = b.union(&c.bbox());
            }
            for &p in &l.points {
                b.include_point(p);
            }
        }
        b
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name)
    }
}

pub fn import_dxf_bytes(bytes: &[u8], opts: &ImportOptions) -> Result<DxfImport, DxfError> {
    // empty/whitespace input would otherwise load as an empty drawing and hide a broken file
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let has_section = lines.windows(2).any(|w| w == ["0", "SECTION"]);
    if !has_section && !bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(DxfError::Parse("no SECTION found".into()));
    }
    // the dxf crate is a third-party parser of untrusted input: turn any panic into a parse error
    let loaded = std::panic::catch_unwind(|| dxf::Drawing::load(&mut std::io::Cursor::new(bytes)))
        .map_err(|_| DxfError::Parse("parser panicked".into()))?;
    let drawing = loaded?;
    let mut imp = entities::import_drawing(&drawing, opts);
    for (name, count) in scan::scan_entity_types(bytes) {
        if !entities::SUPPORTED.contains(&name.as_str()) {
            *imp.ignored.entry(name).or_insert(0) += count;
        }
    }
    Ok(imp)
}

pub fn import_dxf_file(path: &Path, opts: &ImportOptions) -> Result<DxfImport, DxfError> {
    let bytes = std::fs::read(path).map_err(DxfError::Io)?;
    import_dxf_bytes(&bytes, opts)
}
