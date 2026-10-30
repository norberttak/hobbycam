//! M20 acceptance tests: `prefs.json` load/save.
use hc_project::{Prefs, ProjectError};
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hc-m20-prefs-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn load_text(name: &str, text: &str) -> Result<Prefs, ProjectError> {
    let d = scratch(name);
    let f = d.join("prefs.json");
    fs::write(&f, text).unwrap();
    let r = Prefs::load(&f);
    let _ = fs::remove_dir_all(&d);
    r
}

#[test]
fn p1_missing_file_gives_defaults() {
    let d = scratch("p1");
    let f = d.join("prefs.json");
    let p = Prefs::load(&f).unwrap();
    assert_eq!(p, Prefs::default());
    assert_eq!(p.format_version, 1);
    assert!(p.snap_objects && p.snap_grid);
    assert!(!f.exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn p2_save_exact_text_and_roundtrip() {
    let d = scratch("p2");
    let f = d.join("prefs.json");
    let p = Prefs {
        snap_grid: false,
        ..Prefs::default()
    };
    p.save(&f).unwrap();
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "{\n  \"format_version\": 1,\n  \"snap_objects\": true,\n  \"snap_grid\": false\n}\n"
    );
    assert_eq!(Prefs::load(&f).unwrap(), p);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn p3_missing_and_unknown_fields() {
    assert_eq!(
        load_text("p3a", r#"{"format_version":1}"#).unwrap(),
        Prefs::default()
    );
    let p = load_text("p3b", r#"{"format_version":1,"snap_grid":false,"extra":5}"#).unwrap();
    assert!(p.snap_objects && !p.snap_grid);
}

#[test]
fn p4_errors() {
    assert!(matches!(
        load_text("p4a", r#"{"format_version":2,"snap_grid":false}"#),
        Err(ProjectError::VersionTooNew {
            found: 2,
            supported: 1
        })
    ));
    assert!(matches!(
        load_text("p4b", r#"{"snap_grid":false}"#),
        Err(ProjectError::MissingVersion)
    ));
    assert!(matches!(
        load_text("p4c", r#"{"format_version":1,"snap_grid":"yes"}"#),
        Err(ProjectError::Json(_))
    ));
    assert!(matches!(
        load_text("p4d", "{not json"),
        Err(ProjectError::Json(_))
    ));
    assert!(load_text("p4e", "\u{feff}{\"format_version\":1}").is_ok());
}

#[test]
fn p5_second_save_keeps_bak() {
    let d = scratch("p5");
    let f = d.join("prefs.json");
    let first = Prefs::default();
    first.save(&f).unwrap();
    let first_text = fs::read_to_string(&f).unwrap();
    Prefs {
        snap_objects: false,
        ..first
    }
    .save(&f)
    .unwrap();
    assert_eq!(
        fs::read_to_string(d.join("prefs.json.bak")).unwrap(),
        first_text
    );
    let _ = fs::remove_dir_all(&d);
}
