use crate::ProjectError;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current project format version.
pub const FORMAT_VERSION: u32 = 4;
/// Current tool library format version (its schema is unchanged since v1).
pub const LIBRARY_FORMAT_VERSION: u32 = 1;

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = OsString::from(path.as_os_str());
    s.push(suffix);
    PathBuf::from(s)
}

/// Write via `<path>.tmp` + rename; an existing file is first copied to `<path>.bak`.
///
/// Sandbox fallback (Mac App Store build, or a read-only directory with a writable file), only on `PermissionDenied`
/// from the `.bak` copy or the `.tmp` file:
/// - `path` exists: it is rewritten in place; if that write fails the old bytes are put back and the original error is
///   returned. A `.bak` may remain if only the `.tmp` was denied.
/// - `path` does not exist: it is created directly (`create_new`); a partial file is removed if the write fails.
///   In a fully read-only directory creating `path` is denied too, so the save still errors.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let tmp = with_suffix(path, ".tmp");
    match backup_and_create_tmp(path, &tmp) {
        Ok(f) => finish_tmp(f, &tmp, path, bytes),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && path.is_file() => {
            write_in_place(path, bytes, write_synced)
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && !path.exists() => {
            write_new(path, bytes)
        }
        Err(e) => Err(e.into()),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = fs::remove_file(path);
        return Err(e.into());
    }
    Ok(())
}

fn backup_and_create_tmp(path: &Path, tmp: &Path) -> std::io::Result<fs::File> {
    if path.exists() {
        fs::copy(path, with_suffix(path, ".bak"))?;
    }
    fs::File::create(tmp)
}

fn finish_tmp(mut f: fs::File, tmp: &Path, path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(tmp, path)?;
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// In-place rewrite; `write` is injectable so the restore branch can be tested.
fn write_in_place(
    path: &Path,
    bytes: &[u8],
    write: impl Fn(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), ProjectError> {
    let old = fs::read(path)?;
    if let Err(e) = write(path, bytes) {
        // best effort: the original error is what the user needs to see
        let _ = write_synced(path, &old);
        return Err(e.into());
    }
    Ok(())
}

/// Upgrade an older project file's JSON to the current format.
/// v1 → v2: every `operations[].tabs` object gets `"manual": []` (a missing tabs object is left to serde).
/// v2 → v3: `machine.tool_change_z = max(machine.safe_z, 40)` when absent (missing machine/safe_z is left to serde).
/// v3 → v4: no change (new variant only).
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
    if found < 3 {
        if let Some(machine) = value.get_mut("machine").and_then(Value::as_object_mut) {
            if let Some(safe) = machine.get("safe_z").and_then(Value::as_f64) {
                machine
                    .entry("tool_change_z")
                    .or_insert(Value::from(safe.max(40.0)));
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

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hc-io-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_keeps_bak_in_normal_dir() {
        let d = tmp_dir("normal");
        let p = d.join("a.hcam");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read(d.join("a.hcam.bak")).unwrap(), b"one");
        assert!(!d.join("a.hcam.tmp").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_falls_back_in_place_in_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp_dir("ro");
        let p = d.join("a.hcam");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let existing = atomic_write(&p, b"new");
        let fresh = atomic_write(&d.join("b.hcam"), b"x");
        let entries: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        existing.unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert_eq!(
            entries,
            vec![std::ffi::OsString::from("a.hcam")],
            "no .bak, no .tmp"
        );
        match fresh {
            Err(ProjectError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
            other => panic!("new file in a denied dir: {other:?}"),
        }
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn write_in_place_restores_old_bytes_on_failure() {
        let d = tmp_dir("restore");
        let p = d.join("a.ngc");
        fs::write(&p, b"old bytes").unwrap();
        let err = write_in_place(&p, b"new", |p, _| {
            fs::write(p, b"half")?; // truncated + partial, then the disk "fills up"
            Err(std::io::Error::other("disk full"))
        });
        assert!(matches!(err, Err(ProjectError::Io(e)) if e.to_string() == "disk full"));
        assert_eq!(fs::read(&p).unwrap(), b"old bytes");
        write_in_place(&p, b"new", write_synced).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn write_new_creates_but_never_overwrites() {
        let d = tmp_dir("new");
        let p = d.join("n.ngc");
        write_new(&p, b"g0").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"g0");
        assert!(matches!(write_new(&p, b"x"), Err(ProjectError::Io(_))));
        assert_eq!(fs::read(&p).unwrap(), b"g0");
        fs::remove_dir_all(&d).unwrap();
    }

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

    #[test]
    fn migrate_v2_adds_tool_change_z() {
        let m = migrate(json!({"machine": {"safe_z": 5.0}}), 2).unwrap();
        assert_eq!(m["machine"]["tool_change_z"], json!(40.0));
        let m = migrate(json!({"machine": {"safe_z": 50.0}}), 2).unwrap();
        assert_eq!(m["machine"]["tool_change_z"], json!(50.0));
        let keep = json!({"machine": {"safe_z": 5.0, "tool_change_z": 7.0}});
        assert_eq!(migrate(keep.clone(), 2).unwrap(), keep);
        // missing machine / safe_z: left to serde
        let bad = json!({"machine": {"clearance_z": 2.0}});
        assert_eq!(migrate(bad.clone(), 2).unwrap(), bad);
        assert_eq!(migrate(json!({}), 2).unwrap(), json!({}));
        let v3 = json!({"machine": {"safe_z": 5.0}});
        assert_eq!(migrate(v3.clone(), 3).unwrap(), v3);
    }
}
