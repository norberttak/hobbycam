//! Tester edge cases for the M12 `atomic_write` sandbox fallback. Scratch files only under CARGO_TARGET_TMPDIR.
#![cfg(unix)]

use hc_project::{atomic_write, ProjectError};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("hc-project-m12")
        .join(name);
    if d.exists() {
        chmod(&d, 0o755);
        for e in fs::read_dir(&d).unwrap() {
            let _ = fs::set_permissions(e.unwrap().path(), fs::Permissions::from_mode(0o755));
        }
    }
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::canonicalize(&d).unwrap()
}

fn chmod(p: &Path, mode: u32) {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

/// root ignores permission bits; then the permission tests prove nothing
fn perms_enforced(dir: &Path) -> bool {
    chmod(dir, 0o555);
    let w = fs::write(dir.join("probe"), b"x").is_ok();
    chmod(dir, 0o755);
    let _ = fs::remove_file(dir.join("probe"));
    !w
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

#[test]
fn read_only_file_in_read_only_dir_fails_and_keeps_bytes() {
    let d = scratch("ro_file_ro_dir");
    if !perms_enforced(&d) {
        return;
    }
    let p = d.join("a.ngc");
    fs::write(&p, b"old").unwrap();
    chmod(&p, 0o444);
    chmod(&d, 0o555);
    let r = atomic_write(&p, b"new");
    chmod(&d, 0o755);
    chmod(&p, 0o644);
    assert!(
        matches!(&r, Err(ProjectError::Io(e)) if e.kind() == ErrorKind::PermissionDenied),
        "{r:?}"
    );
    assert_eq!(fs::read(&p).unwrap(), b"old");
    assert_eq!(names(&d), ["a.ngc"]);
}

#[test]
fn unreadable_file_in_read_only_dir_is_not_truncated() {
    // write-only file: the fallback cannot save the old bytes, so it must not touch the file
    let d = scratch("wo_file_ro_dir");
    if !perms_enforced(&d) {
        return;
    }
    let p = d.join("a.hcam");
    fs::write(&p, b"precious").unwrap();
    chmod(&p, 0o200);
    chmod(&d, 0o555);
    let r = atomic_write(&p, b"new");
    chmod(&d, 0o755);
    chmod(&p, 0o644);
    assert!(r.is_err(), "{r:?}");
    assert_eq!(fs::read(&p).unwrap(), b"precious");
}

#[test]
fn read_only_stale_bak_in_writable_dir_still_saves() {
    let d = scratch("ro_bak");
    let p = d.join("a.hcam");
    fs::write(&p, b"v1").unwrap();
    fs::write(d.join("a.hcam.bak"), b"v0").unwrap();
    chmod(&d.join("a.hcam.bak"), 0o444);
    let r = atomic_write(&p, b"v2");
    chmod(&d.join("a.hcam.bak"), 0o644);
    if !perms_enforced(&d) {
        return;
    }
    r.unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"v2");
    assert!(!d.join("a.hcam.tmp").exists());
}

#[test]
fn read_only_stale_tmp_in_writable_dir_still_saves() {
    let d = scratch("ro_tmp");
    let p = d.join("a.hcam");
    fs::write(&p, b"v1").unwrap();
    fs::write(d.join("a.hcam.tmp"), b"junk").unwrap();
    chmod(&d.join("a.hcam.tmp"), 0o444);
    let r = atomic_write(&p, b"v2");
    chmod(&d.join("a.hcam.tmp"), 0o644);
    if !perms_enforced(&d) {
        return;
    }
    r.unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"v2");
    assert_eq!(fs::read(d.join("a.hcam.bak")).unwrap(), b"v1");
}

#[test]
fn non_permission_error_does_not_take_fallback() {
    // `<path>.bak` is a directory: copy fails with a non-PermissionDenied kind, the file must stay untouched
    let d = scratch("bak_is_dir");
    let p = d.join("a.hcam");
    fs::write(&p, b"old").unwrap();
    fs::create_dir(d.join("a.hcam.bak")).unwrap();
    let r = atomic_write(&p, b"new");
    match &r {
        Err(ProjectError::Io(e)) => assert_ne!(e.kind(), ErrorKind::PermissionDenied, "{e}"),
        other => panic!("expected an io error, got {other:?}"),
    }
    assert_eq!(fs::read(&p).unwrap(), b"old");
    assert!(!d.join("a.hcam.tmp").exists());
}

#[test]
fn symlink_in_read_only_dir_writes_through_to_target() {
    let d = scratch("symlink");
    let real = d.join("real");
    let links = d.join("links");
    fs::create_dir(&real).unwrap();
    fs::create_dir(&links).unwrap();
    if !perms_enforced(&links) {
        return;
    }
    let target = real.join("a.hcam");
    fs::write(&target, b"old").unwrap();
    let link = links.join("a.hcam");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    chmod(&links, 0o555);
    let r = atomic_write(&link, b"new");
    chmod(&links, 0o755);
    r.unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(names(&links), ["a.hcam"]);
    assert_eq!(names(&real), ["a.hcam"]);
}

#[test]
fn empty_bytes_in_read_only_dir() {
    let d = scratch("empty");
    if !perms_enforced(&d) {
        return;
    }
    let p = d.join("a.ngc");
    fs::write(&p, b"old").unwrap();
    chmod(&d, 0o555);
    let r = atomic_write(&p, b"");
    chmod(&d, 0o755);
    r.unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"");
}

// ---- real sandbox denials (EPERM) via macOS sandbox-exec: only the chosen file is writable, like an NSSavePanel grant

/// Child half: runs only inside `sandbox-exec` (env set by the parent), otherwise a no-op.
#[test]
fn sandbox_child() {
    let Ok(p) = std::env::var("HC_M12_SANDBOX_PATH") else {
        return;
    };
    atomic_write(Path::new(&p), b"new").unwrap();
}

/// Runs `sandbox_child` with write access to `file` only; None if sandbox-exec is unusable here.
#[cfg(target_os = "macos")]
fn run_sandboxed(dir: &Path, file: &Path) -> Option<std::process::Output> {
    use std::process::Command;
    let probe = Command::new("sandbox-exec")
        .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
        .status()
        .ok()?;
    if !probe.success() {
        return None;
    }
    let profile = format!(
        "(version 1)(allow default)(deny file-write* (subpath \"{}\"))(allow file-write* (literal \"{}\"))",
        dir.display(),
        file.display()
    );
    Some(
        Command::new("sandbox-exec")
            .arg("-p")
            .arg(profile)
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sandbox_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HC_M12_SANDBOX_PATH", file)
            .output()
            .unwrap(),
    )
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_save_over_existing_file() {
    let d = scratch("sb_existing");
    let p = d.join("p.hcam");
    fs::write(&p, b"old").unwrap();
    let Some(out) = run_sandboxed(&d, &p) else {
        return;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read(&p).unwrap(), b"new");
    assert_eq!(names(&d), ["p.hcam"], "no .bak/.tmp in the sandbox");
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_save_new_file() {
    let d = scratch("sb_new");
    let p = d.join("p.hcam");
    let Some(out) = run_sandboxed(&d, &p) else {
        return;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read(&p).unwrap(), b"new");
    assert_eq!(names(&d), ["p.hcam"]);
}
