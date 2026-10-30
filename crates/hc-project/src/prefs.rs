use crate::io::{atomic_write, from_versioned_json, no_migrate, to_checked_json};
use crate::ProjectError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Current `prefs.json` format version.
pub const PREFS_FORMAT_VERSION: u32 = 1;

/// App preferences of this computer (not part of a project).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub format_version: u32,
    /// Snap to object key points (ends, corners, centres, …).
    pub snap_objects: bool,
    /// Snap to the visible minor grid when no object point is near.
    pub snap_grid: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            format_version: PREFS_FORMAT_VERSION,
            snap_objects: true,
            snap_grid: true,
        }
    }
}

impl Prefs {
    /// Missing file → defaults. Never writes. Unknown fields are ignored, missing ones take the default.
    pub fn load(path: &Path) -> Result<Prefs, ProjectError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut p: Prefs = from_versioned_json(&text, PREFS_FORMAT_VERSION, no_migrate)?;
                p.format_version = PREFS_FORMAT_VERSION;
                Ok(p)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Prefs::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic write (old file kept as `prefs.json.bak` by `atomic_write`).
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        atomic_write(path, to_checked_json(self)?.as_bytes())
    }
}
