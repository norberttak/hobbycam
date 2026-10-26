use crate::ProjectError;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current project format version.
pub const FORMAT_VERSION: u32 = 2;
/// Current tool library format version (its schema is unchanged since v1).
pub const LIBRARY_FORMAT_VERSION: u32 = 1;

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

/// Upgrade an older project file's JSON to the current format.
/// v1 → v2: every `operations[].tabs` object gets `"manual": []` (a missing tabs object is left to serde).
pub fn migrate(mut value: Value, found: u32) -> Result<Value, ProjectError> {
    if found < 2 {
        if let Some(ops) = value.get_mut("operations").and_then(Value::as_array_mut) {
            for op in ops {
                if let Some(tabs) = op.get_mut("tabs").and_then(Value::as_object_mut) {
                    tabs.entry("manual").or_insert(Value::Array(Vec::new()));
                }
            }
        }
    }
    Ok(value)
}

/// Identity migration for formats that never changed (the tool library).
pub(crate) fn no_migrate(value: Value, _found: u32) -> Result<Value, ProjectError> {
    Ok(value)
}

/// Parse JSON text: version check against `supported` → `migrate` → typed value.
pub(crate) fn from_versioned_json<T: DeserializeOwned>(
    text: &str,
    supported: u32,
    migrate: fn(Value, u32) -> Result<Value, ProjectError>,
) -> Result<T, ProjectError> {
    // editors on Windows may prepend a UTF-8 BOM
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = serde_json::from_str(text)?;
    let found = value
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or(ProjectError::MissingVersion)?;
    let found = u32::try_from(found).unwrap_or(u32::MAX);
    if found > supported {
        return Err(ProjectError::VersionTooNew { found, supported });
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrate_v1_adds_manual_only_to_tabs_objects() {
        let v = json!({"operations": [
            {"tabs": {"enabled": true}},
            {"tabs": {"enabled": false, "manual": [{"item": 1, "positions": [0.5]}]}},
            {"name": "no tabs"}
        ]});
        let m = migrate(v.clone(), 1).unwrap();
        assert_eq!(m["operations"][0]["tabs"]["manual"], json!([]));
        assert_eq!(m["operations"][1], v["operations"][1]);
        assert_eq!(m["operations"][2], v["operations"][2]);
        assert_eq!(migrate(v.clone(), 2).unwrap(), v);
    }
}
