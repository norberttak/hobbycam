use crate::ProjectError;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current project and tool library format version.
pub const FORMAT_VERSION: u32 = 1;

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = OsString::from(path.as_os_str());
    s.push(suffix);
    PathBuf::from(s)
}

/// Write via `<path>.tmp` + rename; an existing file is first copied to `<path>.bak`.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    if path.exists() {
        fs::copy(path, with_suffix(path, ".bak"))?;
    }
    let tmp = with_suffix(path, ".tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Upgrade an older file's JSON to the current format. v1 is current: identity.
pub fn migrate(value: Value, _found: u32) -> Result<Value, ProjectError> {
    Ok(value)
}

/// Parse JSON text: version check → migrate → typed value.
pub(crate) fn from_versioned_json<T: DeserializeOwned>(text: &str) -> Result<T, ProjectError> {
    // editors on Windows may prepend a UTF-8 BOM
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = serde_json::from_str(text)?;
    let found = value
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or(ProjectError::MissingVersion)?;
    let found = u32::try_from(found).unwrap_or(u32::MAX);
    if found > FORMAT_VERSION {
        return Err(ProjectError::VersionTooNew {
            found,
            supported: FORMAT_VERSION,
        });
    }
    Ok(serde_json::from_value(migrate(value, found)?)?)
}

/// Pretty JSON + newline; refuses values that would not load back (NaN/inf become `null`).
pub(crate) fn to_checked_json<T: Serialize + DeserializeOwned>(
    v: &T,
) -> Result<String, ProjectError> {
    let text = serde_json::to_string_pretty(v)? + "\n";
    serde_json::from_str::<T>(&text).map_err(|_| ProjectError::NonFinite)?;
    Ok(text)
}
