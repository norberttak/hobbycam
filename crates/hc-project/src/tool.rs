use crate::io::{atomic_write, from_versioned_json, to_checked_json, FORMAT_VERSION};
use crate::ProjectError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolKind {
    FlatEndMill,
    BallEndMill,
    VBit { angle_deg: f64 },
    Drill,
}

/// A cutting tool. Lengths mm, feeds mm/min.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// Tool number (T<n>), > 0, unique.
    pub id: u32,
    pub name: String,
    pub kind: ToolKind,
    pub diameter: f64,
    pub flute_length: f64,
    pub flutes: u32,
    pub feed: f64,
    pub plunge_feed: f64,
    pub rpm: u32,
    pub depth_per_pass: f64,
    pub stepover_pct: f64,
    #[serde(default)]
    pub comment: String,
}

/// The user's tool inventory, kept sorted by id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolLibrary {
    pub format_version: u32,
    pub tools: Vec<Tool>,
}

impl Default for ToolLibrary {
    fn default() -> Self {
        ToolLibrary {
            format_version: FORMAT_VERSION,
            tools: Vec::new(),
        }
    }
}

impl ToolLibrary {
    /// Missing file → empty library. Never writes.
    pub fn load(path: &Path) -> Result<ToolLibrary, ProjectError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut lib: ToolLibrary = from_versioned_json(&text)?;
                lib.format_version = FORMAT_VERSION;
                lib.tools.sort_by_key(|t| t.id);
                // a duplicated T-number is ambiguous at the manual tool change: refuse the file
                if let Some(w) = lib.tools.windows(2).find(|w| w[0].id == w[1].id) {
                    return Err(ProjectError::DuplicateToolId(w[0].id));
                }
                Ok(lib)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ToolLibrary::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        atomic_write(path, to_checked_json(self)?.as_bytes())
    }

    pub fn get(&self, id: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }

    pub fn add(&mut self, t: Tool) -> Result<(), ProjectError> {
        if t.id == 0 {
            return Err(ProjectError::InvalidToolId);
        }
        match self.tools.binary_search_by_key(&t.id, |x| x.id) {
            Ok(_) => Err(ProjectError::DuplicateToolId(t.id)),
            Err(pos) => {
                self.tools.insert(pos, t);
                Ok(())
            }
        }
    }

    pub fn update(&mut self, t: Tool) -> Result<(), ProjectError> {
        let slot = self
            .tools
            .iter_mut()
            .find(|x| x.id == t.id)
            .ok_or(ProjectError::NoSuchTool(t.id))?;
        *slot = t;
        Ok(())
    }

    pub fn remove(&mut self, id: u32) -> Option<Tool> {
        let pos = self.tools.iter().position(|t| t.id == id)?;
        Some(self.tools.remove(pos))
    }

    pub fn next_free_id(&self) -> u32 {
        self.tools.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }
}

/// `<user config dir>/HobbyCAM/tools.json` (platform specific); None if no home directory.
pub fn default_library_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "HobbyCAM")?;
    Some(dirs.config_dir().join("tools.json"))
}
