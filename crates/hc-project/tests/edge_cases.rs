//! Tester edge cases for M3 (public API only). Scratch files only under CARGO_TARGET_TMPDIR.

use hc_dxf::{import_dxf_file, ImportOptions};
use hc_geom::{Circle, Contour, Point, Segment};
use hc_project::*;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const BRACKET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dxf/autocad2018_bracket_inch.dxf"
);
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/project/small_v1.hcam"
);

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("hc-project-edge")
        .join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn tool(id: u32) -> Tool {
    Tool {
        id,
        name: format!("Flat {id}"),
        kind: ToolKind::FlatEndMill,
        diameter: 3.175,
        flute_length: 12.0,
        flutes: 2,
        feed: 600.0,
        plunge_feed: 150.0,
        rpm: 16000,
        depth_per_pass: 0.8,
        stepover_pct: 45.0,
        comment: String::new(),
    }
}

fn golden_value() -> Value {
    serde_json::from_str(&fs::read_to_string(GOLDEN).unwrap()).unwrap()
}

fn golden_project() -> Project {
    Project::from_json(&fs::read_to_string(GOLDEN).unwrap())
        .unwrap()
        .0
}

fn bracket_project() -> Project {
    let imp = import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap();
    let mut p = Project::new();
    p.add_dxf(&imp, "Bracket.dxf");
    p
}

/// Every f64 of a JSON value tree, in document order, as bits.
fn float_bits(v: &Value, out: &mut Vec<(String, u64)>, path: &str) {
    match v {
        Value::Number(n) if n.is_f64() => {
            out.push((path.to_string(), n.as_f64().unwrap().to_bits()))
        }
        Value::Array(a) => a
            .iter()
            .enumerate()
            .for_each(|(i, x)| float_bits(x, out, &format!("{path}[{i}]"))),
        Value::Object(o) => o
            .iter()
            .for_each(|(k, x)| float_bits(x, out, &format!("{path}.{k}"))),
        _ => {}
    }
}

// ---------------------------------------------------------------- hand-edited files

#[test]
fn missing_defaulted_field_loads() {
    // Tool.comment is #[serde(default)]
    let mut v = golden_value();
    v["tools"][0].as_object_mut().unwrap().remove("comment");
    let (p, problems) = Project::from_json(&v.to_string()).unwrap();
    assert!(problems.is_empty());
    assert_eq!(p.tools[0].comment, "");
    assert_eq!(p.tools[0].diameter, 3.175);
}

#[test]
fn missing_required_field_is_json_error_naming_it() {
    let mut v = golden_value();
    v["operations"][0].as_object_mut().unwrap().remove("tabs");
    match Project::from_json(&v.to_string()) {
        Err(ProjectError::Json(e)) => assert!(e.to_string().contains("tabs"), "{e}"),
        other => panic!("expected Json error, got {other:?}"),
    }
}

#[test]
fn unknown_extra_field_is_ignored() {
    // spec: additive fields need no version bump, so an older app must accept a newer
    // file's extra fields (serde default behaviour: ignored)
    let mut v = golden_value();
    v["future_setting"] = json!({"a": 1});
    v["tools"][0]["coating"] = json!("TiN");
    v["operations"][1]["kind"]["dwell_s"] = json!(0.5);
    let (p, problems) = Project::from_json(&v.to_string()).unwrap();
    assert!(problems.is_empty());
    assert_eq!(p, golden_project());
}

#[test]
fn format_version_variants() {
    let with = |ver: Value| {
        let mut v = golden_value();
        v["format_version"] = ver;
        Project::from_json(&v.to_string())
    };
    // 0: below current -> handed to migrate (v1 hook = identity), loads as v1
    let (p, _) = with(json!(0)).unwrap();
    assert_eq!(p.format_version, FORMAT_VERSION);
    assert_eq!(p, golden_project());
    // not an unsigned integer -> treated as missing
    for bad in [json!(-1), json!("1"), json!(1.5), json!(null), json!([1])] {
        assert!(
            matches!(with(bad.clone()), Err(ProjectError::MissingVersion)),
            "{bad}"
        );
    }
    // way too new, beyond u32
    assert!(matches!(
        with(json!(u64::MAX)),
        Err(ProjectError::VersionTooNew {
            found: u32::MAX,
            supported: 1
        })
    ));
    assert!(matches!(
        with(json!(3)),
        Err(ProjectError::VersionTooNew {
            found: 3,
            supported: 1
        })
    ));
    // top level not an object
    assert!(matches!(
        Project::from_json("[1,2]"),
        Err(ProjectError::MissingVersion)
    ));
}

#[test]
fn version_check_happens_before_shape_check() {
    // a v2 file with an unknown item type must give the version error, not a JSON error
    let mut v = golden_value();
    v["format_version"] = json!(2);
    v["items"][0]["geom"] = json!({"type": "text", "text": "hi"});
    assert!(matches!(
        Project::from_json(&v.to_string()),
        Err(ProjectError::VersionTooNew { found: 2, .. })
    ));
}

#[test]
fn not_json_empty_and_bom() {
    let dir = scratch("not_json");
    let cases: [(&str, &[u8]); 3] = [
        ("text", b"hello world"),
        ("empty", b""),
        ("truncated", br#"{"format_version":1,"#),
    ];
    for (name, bytes) in cases {
        let path = dir.join(format!("{name}.hcam"));
        fs::write(&path, bytes).unwrap();
        let r = Project::load(&path);
        assert!(matches!(r, Err(ProjectError::Json(_))), "{name}: {r:?}");
        let l = ToolLibrary::load(&path);
        assert!(matches!(l, Err(ProjectError::Json(_))), "{name}: {l:?}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "{name} modified");
    }
    // Json error message carries line/column
    let Err(ProjectError::Json(e)) = Project::from_json("{\n  \"format_version\": 1,\n  oops\n}")
    else {
        panic!()
    };
    assert_eq!(e.line(), 3);
    // a leading UTF-8 BOM is stripped (architect R2)
    let path = dir.join("bom.json");
    fs::write(&path, b"\xEF\xBB\xBF{\"format_version\":1,\"tools\":[]}").unwrap();
    assert!(ToolLibrary::load(&path).unwrap().tools.is_empty());
    // non UTF-8 bytes -> error, no panic
    let path = dir.join("latin1.hcam");
    fs::write(&path, b"{\"format_version\":1,\"x\":\"\xE9\"}").unwrap();
    assert!(Project::load(&path).is_err());
}

#[test]
fn corrupt_library_never_modified_and_no_side_files() {
    let dir = scratch("corrupt_lib");
    let cases: [&[u8]; 5] = [
        br#"{"format_version":1,"tools":[{"#,
        br#"{"format_version":9,"tools":[]}"#,
        br#"{"tools":[]}"#,
        b"\x00\x01\x02\xff",
        br#"{"format_version":1,"tools":[{"id":1}]}"#,
    ];
    for (i, bytes) in cases.iter().enumerate() {
        let path = dir.join(format!("tools{i}.json"));
        fs::write(&path, bytes).unwrap();
        assert!(ToolLibrary::load(&path).is_err(), "case {i}");
        assert_eq!(&fs::read(&path).unwrap(), bytes, "case {i}");
        assert!(!dir.join(format!("tools{i}.json.bak")).exists());
        assert!(!dir.join(format!("tools{i}.json.tmp")).exists());
    }
}

// ---------------------------------------------------------------- save errors / backups

#[test]
fn save_into_missing_directory() {
    let dir = scratch("save_missing");
    let target = dir.join("a").join("b").join("p.hcam");
    assert!(matches!(
        golden_project().save(&target),
        Err(ProjectError::Io(_))
    ));
    assert!(!dir.join("a").exists());
    assert!(matches!(
        ToolLibrary::default().save(&target),
        Err(ProjectError::Io(_))
    ));
}

#[cfg(unix)]
#[test]
fn save_into_read_only_directory_leaves_original() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("save_ro");
    let path = dir.join("p.hcam");
    golden_project().save(&path).unwrap();
    let before = fs::read(&path).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    // running as root ignores permissions; then there is nothing to test
    let writable = fs::write(dir.join("probe"), b"x").is_ok();
    let mut p = golden_project();
    p.stock.width = 1.0;
    let r = p.save(&path);
    let r_new = p.save(&dir.join("new.hcam"));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return;
    }
    assert!(matches!(r, Err(ProjectError::Io(_))), "{r:?}");
    assert!(matches!(r_new, Err(ProjectError::Io(_))), "{r_new:?}");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!dir.join("new.hcam").exists());
    assert!(!dir.join("p.hcam.tmp").exists());
}

#[cfg(unix)]
#[test]
fn save_over_read_only_file_in_writable_dir() {
    // rename replaces a read-only target on unix; either way the content must be valid
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("save_ro_file");
    let path = dir.join("p.hcam");
    let p = golden_project();
    p.save(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let mut p2 = p.clone();
    p2.stock.width = 42.0;
    match p2.save(&path) {
        Ok(()) => assert_eq!(Project::load(&path).unwrap().0, p2),
        Err(_) => assert_eq!(Project::load(&path).unwrap().0, p),
    }
}

#[test]
fn save_three_times_bak_holds_previous() {
    let dir = scratch("save_bak");
    let path = dir.join("p.hcam");
    let mut p = golden_project();
    let mut contents = Vec::new();
    for w in [10.0, 20.0, 30.0] {
        p.stock.width = w;
        p.save(&path).unwrap();
        contents.push(fs::read(&path).unwrap());
        if contents.len() >= 2 {
            assert_eq!(
                fs::read(dir.join("p.hcam.bak")).unwrap(),
                contents[contents.len() - 2]
            );
        } else {
            assert!(!dir.join("p.hcam.bak").exists());
        }
        assert!(!dir.join("p.hcam.tmp").exists());
    }
    assert_eq!(Project::load(&path).unwrap().0.stock.width, 30.0);
}

#[test]
fn stale_tmp_file_is_overwritten() {
    let dir = scratch("stale_tmp");
    let path = dir.join("p.hcam");
    fs::write(dir.join("p.hcam.tmp"), b"junk from a crash").unwrap();
    golden_project().save(&path).unwrap();
    assert!(!dir.join("p.hcam.tmp").exists());
    assert_eq!(Project::load(&path).unwrap().0, golden_project());
}

#[test]
fn infinite_values_refused() {
    let dir = scratch("inf");
    let path = dir.join("p.hcam");
    for f in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let mut p = golden_project();
        p.items[1].geom = Geom::Point(Point::new(f, 0.0));
        assert!(matches!(p.to_json(), Err(ProjectError::NonFinite)));
        assert!(matches!(p.save(&path), Err(ProjectError::NonFinite)));
        assert!(!path.exists());
        let mut lib = ToolLibrary::default();
        lib.add(Tool {
            diameter: f,
            ..tool(1)
        })
        .unwrap();
        assert!(matches!(lib.save(&path), Err(ProjectError::NonFinite)));
        assert!(!path.exists());
    }
}

// ---------------------------------------------------------------- tool ids

#[test]
fn tool_zero_and_duplicates_in_project_file() {
    let mut v = golden_value();
    let t = v["tools"][0].clone();
    v["tools"].as_array_mut().unwrap().push(t);
    let (_, problems) = Project::from_json(&v.to_string()).unwrap();
    assert_eq!(problems, [Problem::DuplicateToolId(1)]);

    // an op referencing T0 that is not embedded -> MissingTool, still loads
    let mut v = golden_value();
    v["operations"][0]["tool"] = json!(0);
    let (_, problems) = Project::from_json(&v.to_string()).unwrap();
    assert_eq!(problems, [Problem::MissingTool { op: 3, tool: 0 }]);
}

#[test]
fn library_file_with_duplicate_ids_loads_sorted_and_add_still_refuses() {
    let dir = scratch("lib_dups");
    let path = dir.join("tools.json");
    let lib = json!({"format_version": 1, "tools": [tool(5), tool(2), tool(5)]});
    fs::write(&path, lib.to_string()).unwrap();
    // architect R3: a library with duplicate ids is refused and left untouched
    assert!(matches!(
        ToolLibrary::load(&path),
        Err(ProjectError::DuplicateToolId(5))
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), lib.to_string());
    let lib = json!({"format_version": 1, "tools": [tool(5), tool(2)]});
    fs::write(&path, lib.to_string()).unwrap();
    let mut l = ToolLibrary::load(&path).unwrap();
    let ids: Vec<u32> = l.tools.iter().map(|t| t.id).collect();
    assert_eq!(ids, [2, 5]);
    assert!(matches!(
        l.add(tool(5)),
        Err(ProjectError::DuplicateToolId(5))
    ));
    assert_eq!(l.next_free_id(), 6);
    assert!(matches!(l.add(tool(0)), Err(ProjectError::InvalidToolId)));
    l.add(tool(3)).unwrap();
    let ids: Vec<u32> = l.tools.iter().map(|t| t.id).collect();
    assert_eq!(ids, [2, 3, 5]);
}

#[test]
fn library_add_keeps_sorted_with_many_ids() {
    let mut l = ToolLibrary::default();
    for id in [9, 3, 7, 1, 8, 2, u32::MAX - 1] {
        l.add(tool(id)).unwrap();
    }
    let ids: Vec<u32> = l.tools.iter().map(|t| t.id).collect();
    assert_eq!(ids, [1, 2, 3, 7, 8, 9, u32::MAX - 1]);
    assert_eq!(l.next_free_id(), u32::MAX);
    assert_eq!(l.remove(42), None);
}

// ---------------------------------------------------------------- items / operations

#[test]
fn remove_item_cleans_every_operation() {
    let mut p = bracket_project();
    let t = tool(1);
    let a = p.add_operation("a", OpKind::Engrave, &t, vec![3, 5, 3, 7]);
    let b = p.add_operation("b", OpKind::Drill { peck: None }, &t, vec![5]);
    let c = p.add_operation("c", OpKind::Pocket { stepover_pct: 0.0 }, &t, vec![1, 2]);
    p.remove_item(3);
    p.remove_item(5);
    let items = |id: u64| {
        p.operations
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .items
            .clone()
    };
    assert_eq!(items(a), [7]);
    assert!(items(b).is_empty());
    assert_eq!(items(c), [1, 2]);
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    // removing an unknown id is a no-op
    let before = p.clone();
    p.remove_item(12345);
    assert_eq!(p, before);
}

#[test]
fn add_operation_copies_tool_and_later_library_change_does_not_leak() {
    let mut lib = ToolLibrary::default();
    lib.add(tool(4)).unwrap();
    let mut p = Project::new();
    let op = p.add_operation("x", OpKind::Engrave, lib.get(4).unwrap(), vec![]);
    lib.update(Tool {
        feed: 1.0,
        rpm: 1,
        diameter: 9.9,
        ..tool(4)
    })
    .unwrap();
    let o = p.operations.iter().find(|o| o.id == op).unwrap();
    assert_eq!((o.feed, o.rpm, o.tool), (600.0, 16000, 4));
    assert_eq!(p.tools, [tool(4)]);
    // architect R4: a second op with the changed tool replaces the embedded copy; op 1 keeps its values
    p.add_operation("y", OpKind::Engrave, lib.get(4).unwrap(), vec![]);
    assert_eq!(p.tools, [lib.get(4).unwrap().clone()]);
    let o = p.operations.iter().find(|o| o.id == op).unwrap();
    assert_eq!((o.feed, o.rpm), (600.0, 16000));
    // non-pocket kinds are kept as given
    let k = OpKind::Contour {
        side: Side::Inside,
        direction: Direction::Conventional,
    };
    let id = p.add_operation("z", k.clone(), &tool(4), vec![]);
    assert_eq!(p.operations.iter().find(|o| o.id == id).unwrap().kind, k);
    assert_eq!(p.operations.last().unwrap().tabs, Tabs::default());
    assert!(p.operations.last().unwrap().enabled);
}

#[test]
fn prune_tools_drops_only_unused() {
    let mut p = Project::new();
    p.add_operation("a", OpKind::Engrave, &tool(1), vec![]);
    let b = p.add_operation("b", OpKind::Engrave, &tool(2), vec![]);
    p.remove_operation(b);
    assert_eq!(p.unused_tools(), [2]);
    p.prune_tools();
    let ids: Vec<u32> = p.tools.iter().map(|t| t.id).collect();
    assert_eq!(ids, [1]);
}

#[test]
fn ids_never_reused_after_delete_and_add() {
    let mut p = bracket_project(); // ids 1..=29
    let t = tool(1);
    let op = p.add_operation("a", OpKind::Engrave, &t, vec![29]); // 30
    assert_eq!(op, 30);
    p.remove_operation(op);
    p.remove_item(29);
    p.remove_item(28);
    let op2 = p.add_operation("b", OpKind::Engrave, &t, vec![]);
    assert_eq!(op2, 31);
    let imp = import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap();
    let new = p.add_dxf(&imp, "again.dxf");
    assert_eq!(new, (32..=60).collect::<Vec<u64>>());
    assert_eq!(p.next_id, 61);
    // survives a save/load cycle
    let (mut back, problems) = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert!(problems.is_empty());
    assert_eq!(back.add_operation("c", OpKind::Engrave, &t, vec![]), 61);
    // layers not duplicated by the second import
    assert_eq!(back.layers.len(), 4);
}

// ---------------------------------------------------------------- units

#[test]
fn inch_display_round_trip_stores_mm() {
    let mut p = golden_project();
    p.units = Units::Inch;
    p.stock.width = from_display(4.0, Units::Inch);
    p.operations[0].depth = parse_length("1/4", p.units).unwrap();
    let text = p.to_json().unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["units"], json!("inch"));
    assert_eq!(v["stock"]["width"].as_f64(), Some(101.6));
    assert_eq!(v["operations"][0]["depth"].as_f64(), Some(6.35));
    assert_eq!(v["tools"][0]["diameter"].as_f64(), Some(3.175));
    let (back, _) = Project::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(to_display(back.stock.width, back.units), 4.0);
    assert_eq!(to_display(back.tools[0].diameter, Units::Inch), 0.125);
}

#[test]
fn parse_length_cases() {
    let eq = |s: &str, u: Units, e: f64| {
        let v = parse_length(s, u);
        assert!(
            v.is_some_and(|v| (v - e).abs() < 1e-12),
            "{s:?} {u:?} -> {v:?}"
        );
    };
    eq("1/4", Units::Inch, 6.35);
    eq("1/4", Units::Mm, 0.25);
    eq("1 1/2in", Units::Mm, 38.1);
    eq("1 1/2 in", Units::Mm, 38.1);
    eq("6,35", Units::Mm, 6.35);
    eq("6,35", Units::Inch, 6.35 * 25.4);
    eq("6.35mm", Units::Inch, 6.35);
    eq("6.35 MM", Units::Inch, 6.35);
    eq("0.25 in", Units::Mm, 6.35);
    eq("0.25In", Units::Mm, 6.35);
    eq("1\"", Units::Mm, 25.4);
    eq("1 \"", Units::Mm, 25.4);
    eq("-1", Units::Mm, -1.0);
    eq("-1", Units::Inch, -25.4);
    eq("-1/4", Units::Inch, -6.35);
    eq("  3  ", Units::Mm, 3.0);
    eq(".5", Units::Mm, 0.5);
    eq("1/2mm", Units::Inch, 0.5);
    for s in [
        "", " ", "abc", "1/0", "0/0", "1/", "/4", "mm", "in", "\"", "5 mmx", "1 2", "1/4/2",
        "1,000.5", "nan", "inf", "-inf", "1 1/0", "1 1/2 3", "6.35 cm", "6.35m", "1/4 1/2",
    ] {
        assert_eq!(parse_length(s, Units::Mm), None, "{s:?}");
    }
}

#[test]
fn parse_length_very_long_input() {
    // overflow to infinity -> None
    assert_eq!(parse_length(&"9".repeat(400), Units::Mm), None);
    assert_eq!(
        parse_length(&format!("{}in", "9".repeat(400)), Units::Mm),
        None
    );
    // a long but valid decimal
    let long = format!("1.{}", "0".repeat(100_000));
    assert_eq!(parse_length(&long, Units::Mm), Some(1.0));
    // 1 MB of garbage and of whitespace: no panic, None
    assert_eq!(parse_length(&"x".repeat(1 << 20), Units::Mm), None);
    assert_eq!(parse_length(&" ".repeat(1 << 20), Units::Mm), None);
    assert_eq!(parse_length(&"1/".repeat(1 << 18), Units::Mm), None);
}

#[test]
fn display_conversion_is_exact_inverse_for_common_values() {
    for mm in [25.4, 3.175, 6.35, 12.7, 0.0, 1000.0] {
        assert_eq!(from_display(to_display(mm, Units::Inch), Units::Inch), mm);
        assert_eq!(to_display(mm, Units::Mm), mm);
    }
}

// ---------------------------------------------------------------- round trips / JSON shape

#[test]
fn bracket_round_trip_bit_identical() {
    let mut p = bracket_project();
    p.add_operation("all", OpKind::Engrave, &tool(1), (1..=29).collect());
    p.items.push(Item {
        id: p.next_id,
        layer: "neg".into(),
        source: Source::Drawn,
        geom: Geom::Point(Point::new(-0.0, 1e-310)), // negative zero and a subnormal
    });
    p.next_id += 1;
    let text = p.to_json().unwrap();
    let (back, problems) = Project::from_json(&text).unwrap();
    assert!(problems.is_empty());
    // PartialEq treats -0.0 == 0.0, so compare raw bits of every float too
    let (mut a, mut b) = (Vec::new(), Vec::new());
    float_bits(&serde_json::to_value(&p).unwrap(), &mut a, "");
    float_bits(&serde_json::to_value(&back).unwrap(), &mut b, "");
    assert_eq!(a, b);
    let Geom::Point(q) = back.items.last().unwrap().geom else {
        panic!()
    };
    assert_eq!(q.x.to_bits(), (-0.0f64).to_bits());
    assert_eq!(q.y.to_bits(), 1e-310f64.to_bits());
    for (x, y) in p.items.iter().zip(&back.items) {
        if let (Geom::Contour(c1), Geom::Contour(c2)) = (&x.geom, &y.geom) {
            for (s1, s2) in c1.segments.iter().zip(&c2.segments) {
                assert_eq!(s1.start().x.to_bits(), s2.start().x.to_bits());
                assert_eq!(s1.end().y.to_bits(), s2.end().y.to_bits());
            }
        }
    }
    // second save is byte-identical to the first
    assert_eq!(back.to_json().unwrap(), text);
}

#[test]
fn a2_segment_json_shape() {
    let line = Segment::Line {
        start: Point::new(1.0, 2.0),
        end: Point::new(3.0, 4.0),
    };
    assert_eq!(
        serde_json::to_value(line).unwrap(),
        json!({"type": "line", "start": {"x": 1.0, "y": 2.0}, "end": {"x": 3.0, "y": 4.0}})
    );
    let arc = Segment::Arc {
        center: Point::new(0.0, 0.0),
        radius: 2.0,
        start_angle: 0.5,
        sweep: -1.0,
    };
    assert_eq!(
        serde_json::to_value(arc).unwrap(),
        json!({"type": "arc", "center": {"x": 0.0, "y": 0.0}, "radius": 2.0, "start_angle": 0.5, "sweep": -1.0})
    );
    let c = Circle {
        center: Point::new(1.0, 1.0),
        radius: 1.0,
    }
    .to_contour();
    let g = serde_json::to_value(Geom::Contour(c.clone())).unwrap();
    assert_eq!(g["type"], json!("contour"));
    assert_eq!(g["closed"], json!(true));
    assert_eq!(g["segments"][0]["type"], json!("arc"));
    assert_eq!(
        serde_json::to_value(Geom::Point(Point::new(5.0, 6.0))).unwrap(),
        json!({"type": "point", "x": 5.0, "y": 6.0})
    );
    // round trip and rejection of the old externally-tagged form
    assert_eq!(
        serde_json::from_value::<Segment>(
            json!({"type":"line","start":{"x":1.0,"y":2.0},"end":{"x":3.0,"y":4.0}})
        )
        .unwrap(),
        line
    );
    assert!(serde_json::from_value::<Segment>(
        json!({"Line": {"start": {"x":1.0,"y":2.0}, "end": {"x":3.0,"y":4.0}}})
    )
    .is_err());
    assert_eq!(
        serde_json::from_value::<Contour>(serde_json::to_value(&c).unwrap()).unwrap(),
        c
    );
}

#[test]
fn golden_file_json_shape_matches_spec_names() {
    let v = golden_value();
    let keys = |x: &Value| {
        let mut k: Vec<String> = x.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(
        keys(&v),
        [
            "format_version",
            "items",
            "layers",
            "machine",
            "next_id",
            "operations",
            "stock",
            "tools",
            "units"
        ]
    );
    assert_eq!(
        keys(&v["stock"]),
        ["height", "min_x", "min_y", "origin", "thickness", "width"]
    );
    assert_eq!(keys(&v["machine"]), ["clearance_z", "post", "safe_z"]);
    assert_eq!(
        keys(&v["tools"][0]),
        [
            "comment",
            "depth_per_pass",
            "diameter",
            "feed",
            "flute_length",
            "flutes",
            "id",
            "kind",
            "name",
            "plunge_feed",
            "rpm",
            "stepover_pct"
        ]
    );
    assert_eq!(
        keys(&v["operations"][0]),
        [
            "depth",
            "depth_per_pass",
            "enabled",
            "feed",
            "id",
            "items",
            "kind",
            "name",
            "plunge_feed",
            "rpm",
            "tabs",
            "tool"
        ]
    );
    assert_eq!(
        v["operations"][1]["kind"],
        json!({"type": "drill", "peck": 1.5})
    );
    assert_eq!(v["operations"][1]["depth"], json!(6.0)); // A3
    assert_eq!(v["items"][0]["source"], json!({"type": "drawn"}));
    // other enum spellings
    let vb = serde_json::to_value(ToolKind::VBit { angle_deg: 60.0 }).unwrap();
    assert_eq!(vb, json!({"type": "v_bit", "angle_deg": 60.0}));
    assert_eq!(
        serde_json::to_value(Source::Imported {
            file: "a.dxf".into()
        })
        .unwrap(),
        json!({"type": "imported", "file": "a.dxf"})
    );
    assert_eq!(
        serde_json::to_value(WorkOrigin::StockCenter).unwrap(),
        json!("stock_center")
    );
    assert_eq!(
        serde_json::to_value(TabPlacement::Spacing { distance: 50.0 }).unwrap(),
        json!({"type": "spacing", "distance": 50.0})
    );
    assert_eq!(
        serde_json::to_value(OpKind::Drill { peck: None }).unwrap(),
        json!({"type": "drill", "peck": null})
    );
}

#[test]
fn drill_peck_none_round_trips() {
    let mut p = golden_project();
    p.operations[1].kind = OpKind::Drill { peck: None };
    let (back, _) = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(back, p);
}
