//! HobbyCAM project file (`.hcam` JSON) and tool inventory.

mod io;
mod project;
mod tool;
mod units;

pub use io::{atomic_write, migrate, FORMAT_VERSION, LIBRARY_FORMAT_VERSION};
pub use project::{
    Direction, Geom, Item, LayerInfo, Machine, ManualTabs, OpKind, Operation, Problem, Project,
    Side, Source, Stock, TabPlacement, Tabs, WorkOrigin,
};
pub use tool::{default_library_path, Tool, ToolKind, ToolLibrary};
pub use units::{from_display, parse_length, to_display, Units};

use std::fmt;

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingVersion,
    VersionTooNew {
        found: u32,
        supported: u32,
    },
    /// A value is NaN or infinite and cannot be stored.
    NonFinite,
    DuplicateToolId(u32),
    InvalidToolId,
    NoSuchTool(u32),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "file error: {e}"),
            ProjectError::Json(e) => write!(f, "invalid file content: {e}"),
            ProjectError::MissingVersion => write!(f, "format_version is missing or is not a whole number"),
            ProjectError::VersionTooNew { found, supported } => write!(
                f,
                "the file has format version {found}, this HobbyCAM supports up to {supported}; please update HobbyCAM"
            ),
            ProjectError::NonFinite => write!(f, "a value is not a finite number (NaN or infinity)"),
            ProjectError::DuplicateToolId(id) => write!(f, "tool T{id} already exists"),
            ProjectError::InvalidToolId => write!(f, "tool number must be greater than 0"),
            ProjectError::NoSuchTool(id) => write!(f, "tool T{id} does not exist"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Json(e)
    }
}
