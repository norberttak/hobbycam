//! M3 acceptance tests (specs/M3.md), one test per item. Scratch files only under CARGO_TARGET_TMPDIR.

use hc_dxf::{import_dxf_file, ImportOptions};
use hc_geom::{Contour, Point, Segment};
use hc_project::*;
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
const GOLDEN_V2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/project/small_v2.hcam"
);

/// Fresh empty scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("hc-project")
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

fn bracket_project() -> Project {
    let imp = import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap();
    let mut p = Project::new();
    p.add_dxf(&imp, "Bracket.dxf");
    p
}

fn contour(it: &Item) -> &Contour {
    match &it.geom {
        Geom::Contour(c) => c,
        Geom::Point(_) => panic!("not a contour"),
    }
}

fn is_hole(it: &Item) -> bool {
    matches!(&it.geom, Geom::Contour(c) if matches!(c.segments.as_slice(),
        [Segment::Arc { radius, .. }] if (radius - 7.9375).abs() < 1e-6))
}

/// The project stored in tests/fixtures/project/small_v1.hcam.
fn small_project() -> Project {
    let mut p = Project::new();
    let sq = |x0: f64, y0: f64, x1: f64, y1: f64| Segment::Line {
        start: Point::new(x0, y0),
        end: Point::new(x1, y1),
    };
    let square = Contour {
        segments: vec![
            sq(0.0, 0.0, 10.0, 0.0),
            sq(10.0, 0.0, 10.0, 10.0),
            sq(10.0, 10.0, 0.0, 10.0),
            sq(0.0, 10.0, 0.0, 0.0),
        ],
        closed: true,
    };
    p.layers.push(LayerInfo {
        name: "0".into(),
        color: 7,
    });
    for geom in [Geom::Contour(square), Geom::Point(Point::new(5.0, 5.0))] {
        let id = p.next_id;
        p.next_id += 1;
        p.items.push(Item {
            id,
            layer: "0".into(),
            source: Source::Drawn,
            geom,
        });
    }
    let t1 = Tool {
        name: "Flat 1/8\"".into(),
        comment: "2 flute carbide".into(),
        ..tool(1)
    };
    let c = p.add_operation(
        "Cut out",
        OpKind::Contour {
            side: Side::Outside,
            direction: Direction::Climb,
        },
        &t1,
        vec![1],
    );
    let op = p.operations.iter_mut().find(|o| o.id == c).unwrap();
    op.depth = 6.0;
    op.tabs.enabled = true;
    let d = p.add_operation(
        "Center hole",
        OpKind::Drill { peck: Some(1.5) },
        &t1,
        vec![2],
    );
    p.operations.iter_mut().find(|o| o.id == d).unwrap().depth = 6.0;
    p
}

#[test]
fn t01_bracket_add_dxf() {
    let p = bracket_project();
    assert_eq!(p.items.len(), 29);
    let ids: Vec<u64> = p.items.iter().map(|i| i.id).collect();
    assert_eq!(ids, (1..=29).collect::<Vec<u64>>());
    assert_eq!(p.next_id, 30);
    let layers: Vec<(&str, i16)> = p
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.color))
        .collect();
    assert_eq!(
        layers,
        [("Outline", 7), ("Center", 1), ("Hidden", 2), ("Title", 7)]
    );
    let count = |l: &str| p.items.iter().filter(|i| i.layer == l).count();
    assert_eq!(
        (
            count("Outline"),
            count("Center"),
            count("Hidden"),
            count("Title")
        ),
        (12, 11, 1, 5)
    );
    assert!(p.items.iter().all(|i| i.source
        == Source::Imported {
            file: "Bracket.dxf".into()
        }));
}

#[test]
fn t02_bracket_round_trip() {
    let mut p = bracket_project();
    let outer = p
        .items
        .iter()
        .find(|i| contour(i).segments.len() == 8)
        .unwrap()
        .id;
    let holes: Vec<u64> = p
        .items
        .iter()
        .filter(|i| is_hole(i))
        .map(|i| i.id)
        .collect();
    assert_eq!(holes.len(), 4);
    let t1 = tool(1);
    p.add_operation(
        "Pocket",
        OpKind::Pocket { stepover_pct: 0.0 },
        &t1,
        vec![outer],
    );
    p.add_operation("Holes", OpKind::Drill { peck: None }, &t1, holes);
    let (back, problems) = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(back, p);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn t03_float_exactness() {
    let mut p = Project::new();
    let t = Tool {
        diameter: 0.1 + 0.2,
        feed: 1.0 / 3.0,
        ..tool(1)
    };
    let id = p.add_operation("x", OpKind::Engrave, &t, vec![]);
    p.operations[0].depth = 25.4 * 0.3125;
    p.stock.thickness = 1e-300;
    let (b, _) = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(b.tools[0].diameter.to_bits(), (0.1f64 + 0.2).to_bits());
    assert_eq!(b.tools[0].feed.to_bits(), (1.0f64 / 3.0).to_bits());
    assert_eq!(b.operations[0].feed.to_bits(), (1.0f64 / 3.0).to_bits());
    assert_eq!(b.operations[0].id, id);
    assert_eq!(
        b.operations[0].depth.to_bits(),
        (25.4f64 * 0.3125).to_bits()
    );
    assert_eq!(b.stock.thickness.to_bits(), 1e-300f64.to_bits());
}

#[test]
fn t04_golden_file() {
    let text = fs::read_to_string(GOLDEN).unwrap();
    let p = small_project();
    let (loaded, problems) = Project::from_json(&text).unwrap();
    assert!(problems.is_empty());
    assert_eq!(loaded, p);
    // M6: v1 is no longer the current format; the byte-exact check moved to the v2 golden (m6_12)
    assert_eq!(loaded.format_version, 2);
}

#[test]
fn t05_version_check() {
    let mut v: serde_json::Value =
        serde_json::from_str(&small_project().to_json().unwrap()).unwrap();
    v["format_version"] = 3.into(); // M6: current is 2
    assert!(matches!(
        Project::from_json(&v.to_string()),
        Err(ProjectError::VersionTooNew {
            found: 3,
            supported: 2
        })
    ));
    v.as_object_mut().unwrap().remove("format_version");
    assert!(matches!(
        Project::from_json(&v.to_string()),
        Err(ProjectError::MissingVersion)
    ));
    assert_eq!(migrate(v.clone(), 2).unwrap(), v); // M6: v1 now migrates (adds tabs.manual)
}

#[test]
fn t06_unknown_item_type_refused() {
    let mut v: serde_json::Value =
        serde_json::from_str(&small_project().to_json().unwrap()).unwrap();
    v["items"][0]["geom"] = serde_json::json!({"type": "text", "text": "hello"});
    match Project::from_json(&v.to_string()) {
        Err(ProjectError::Json(e)) => assert!(e.to_string().contains("text"), "{e}"),
        other => panic!("expected Json error, got {other:?}"),
    }
}

#[test]
fn t07_dangling_references_are_problems() {
    let mut p = small_project();
    let op = p.operations[0].id;
    p.operations[0].tool = 9;
    p.operations[0].items = vec![999];
    let (_, problems) = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(
        problems,
        [
            Problem::MissingTool { op, tool: 9 },
            Problem::MissingItem { op, item: 999 }
        ]
    );

    let mut p = small_project();
    p.items[1].id = p.items[0].id;
    let (_, problems) = Project::from_json(&p.to_json().unwrap()).unwrap();
    // the drill op now also misses item 2; the duplicate itself is reported once
    let dup = p.items[0].id;
    let id_problems: Vec<&Problem> = problems
        .iter()
        .filter(|q| matches!(q, Problem::DuplicateId(_) | Problem::IdNotBelowNextId(_)))
        .collect();
    assert_eq!(id_problems, [&Problem::DuplicateId(dup)]);

    // a duplicated id that is also >= next_id is reported once
    let mut p = small_project();
    p.items[0].id = 50;
    p.items[1].id = 50;
    p.operations[0].items = vec![50];
    p.operations[1].items = vec![50];
    assert_eq!(p.validate(), [Problem::IdNotBelowNextId(50)]);
}

#[test]
fn t08_remove_item_and_no_id_reuse() {
    let mut p = bracket_project();
    let t = tool(1);
    p.add_operation("all", OpKind::Engrave, &t, vec![1, 2, 29]);
    p.remove_item(29);
    assert_eq!(p.operations[0].items, [1, 2]);
    assert!(p.items.iter().all(|i| i.id != 29));
    let imp = import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap();
    let new = p.add_dxf(&imp, "again.dxf");
    assert!(new.iter().all(|&id| id > 30));
    assert!(p.validate().is_empty());
}

#[test]
fn t09_add_operation_copies_tool_data() {
    let t2 = Tool {
        feed: 800.0,
        plunge_feed: 200.0,
        rpm: 18000,
        depth_per_pass: 1.0,
        stepover_pct: 40.0,
        ..tool(2)
    };
    let mut p = Project::new();
    p.add_operation("a", OpKind::Pocket { stepover_pct: 99.0 }, &t2, vec![]);
    p.add_operation("b", OpKind::Engrave, &t2, vec![]);
    let op = &p.operations[0];
    assert_eq!(
        (op.feed, op.plunge_feed, op.rpm, op.depth_per_pass),
        (800.0, 200.0, 18000, 1.0)
    );
    assert_eq!(op.depth, 1.0);
    assert_eq!(op.kind, OpKind::Pocket { stepover_pct: 40.0 });
    assert_eq!(p.tools.iter().filter(|t| t.id == 2).count(), 1);
    assert_eq!(p.tools.len(), 1);
}

#[test]
fn t10_tool_library() {
    let dir = scratch("t10");
    let path = dir.join("tools.json");
    let mut lib = ToolLibrary::load(&path).unwrap();
    assert!(lib.tools.is_empty());
    for id in [5, 1, 2] {
        lib.add(tool(id)).unwrap();
    }
    let ids: Vec<u32> = lib.tools.iter().map(|t| t.id).collect();
    assert_eq!(ids, [1, 2, 5]);
    assert_eq!(lib.next_free_id(), 6);
    assert!(matches!(
        lib.add(tool(2)),
        Err(ProjectError::DuplicateToolId(2))
    ));
    assert!(matches!(lib.add(tool(0)), Err(ProjectError::InvalidToolId)));
    assert!(matches!(
        lib.update(tool(7)),
        Err(ProjectError::NoSuchTool(7))
    ));
    lib.update(Tool {
        rpm: 1000,
        ..tool(5)
    })
    .unwrap();
    assert_eq!(lib.get(5).unwrap().rpm, 1000);
    lib.save(&path).unwrap();
    assert_eq!(ToolLibrary::load(&path).unwrap(), lib);
    assert_eq!(lib.remove(2).map(|t| t.id), Some(2));
    assert!(lib.get(2).is_none());
    assert_eq!(ToolLibrary::default().next_free_id(), 1);
}

#[test]
fn t11_corrupt_library_untouched_then_backed_up() {
    let dir = scratch("t11");
    let path = dir.join("tools.json");
    let corrupt = br#"{"format_version":1,"tools":[{"#;
    fs::write(&path, corrupt).unwrap();
    assert!(matches!(
        ToolLibrary::load(&path),
        Err(ProjectError::Json(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), corrupt);
    let mut lib = ToolLibrary::default();
    lib.add(tool(3)).unwrap();
    lib.save(&path).unwrap();
    assert_eq!(ToolLibrary::load(&path).unwrap(), lib);
    assert_eq!(fs::read(dir.join("tools.json.bak")).unwrap(), corrupt);
}

#[test]
fn t12_atomic_save() {
    let dir = scratch("t12");
    let path = dir.join("p.hcam");
    let p = small_project();
    p.save(&path).unwrap();
    assert!(!dir.join("p.hcam.tmp").exists());
    let first = fs::read(&path).unwrap();
    let mut p2 = p.clone();
    p2.stock.width = 200.0;
    p2.save(&path).unwrap();
    assert_eq!(fs::read(dir.join("p.hcam.bak")).unwrap(), first);
    assert_eq!(Project::load(&path).unwrap().0, p2);
    assert!(!dir.join("p.hcam.tmp").exists());

    let missing_dir = dir.join("nope");
    assert!(matches!(
        p.save(&missing_dir.join("x.hcam")),
        Err(ProjectError::Io(_))
    ));
    assert!(!missing_dir.exists());
}

#[test]
fn t13_nan_refused_target_unchanged() {
    let dir = scratch("t13");
    let path = dir.join("p.hcam");
    let p = small_project();
    p.save(&path).unwrap();
    let before = fs::read(&path).unwrap();
    let mut bad = p.clone();
    bad.operations[0].depth = f64::NAN;
    assert!(matches!(bad.save(&path), Err(ProjectError::NonFinite)));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn t14_units_and_parse_length() {
    let close = |v: Option<f64>, e: f64| v.is_some_and(|v| (v - e).abs() < 1e-12);
    assert!(close(parse_length("6.35mm", Units::Inch), 6.35));
    for s in ["0.25in", "1/4\"", "1/4"] {
        assert!(close(parse_length(s, Units::Inch), 6.35), "{s}");
    }
    assert!(close(parse_length("1 1/2", Units::Inch), 38.1));
    assert!(close(parse_length("6,35", Units::Mm), 6.35));
    assert!(close(parse_length("3", Units::Mm), 3.0));
    for s in ["abc", "1/0", "", "5 mmx"] {
        assert_eq!(parse_length(s, Units::Mm), None, "{s}");
    }
    assert_eq!(to_display(25.4, Units::Inch), 1.0);
}

#[test]
fn t15_default_library_path() {
    if let Some(p) = default_library_path() {
        assert!(p.ends_with("tools.json"));
        // Linux lowercases the app dir (~/.config/hobbycam)
        assert!(p.to_string_lossy().to_lowercase().contains("hobbycam"));
    }
}

// ---------------------------------------------------------------- architect review

#[test]
fn r2_utf8_bom_is_accepted() {
    let text = format!("\u{feff}{}", small_project().to_json().unwrap());
    assert_eq!(Project::from_json(&text).unwrap().0, small_project());
    let dir = scratch("r2");
    let path = dir.join("tools.json");
    fs::write(&path, "\u{feff}{\"format_version\": 1, \"tools\": []}").unwrap();
    assert!(ToolLibrary::load(&path).unwrap().tools.is_empty());
}

#[test]
fn r3_library_with_duplicate_tool_ids_is_refused() {
    let dir = scratch("r3");
    let path = dir.join("tools.json");
    let lib = ToolLibrary {
        format_version: 1,
        tools: vec![tool(5), tool(2), tool(5)],
    };
    let bytes = serde_json::to_vec(&lib).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        ToolLibrary::load(&path),
        Err(ProjectError::DuplicateToolId(5))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn r4_add_operation_replaces_embedded_tool() {
    let mut lib = ToolLibrary::default();
    lib.add(tool(2)).unwrap();
    let mut p = Project::new();
    let op1 = p.add_operation("a", OpKind::Engrave, lib.get(2).unwrap(), vec![]);
    lib.update(Tool {
        diameter: 6.0,
        feed: 900.0,
        ..tool(2)
    })
    .unwrap();
    let op2 = p.add_operation("b", OpKind::Engrave, lib.get(2).unwrap(), vec![]);
    assert_eq!(p.tools.len(), 1);
    assert_eq!((p.tools[0].diameter, p.tools[0].feed), (6.0, 900.0));
    let feed = |id: u64| p.operations.iter().find(|o| o.id == id).unwrap().feed;
    assert_eq!((feed(op1), feed(op2)), (600.0, 900.0));
}

#[test]
fn r5_missing_version_message() {
    assert_eq!(
        ProjectError::MissingVersion.to_string(),
        "format_version is missing or is not a whole number"
    );
}

// ---------------------------------------------------------------- M6 (specs/M6.md test 12)

/// small_project plus manual tabs on the Contour op (the v2 golden file).
fn small_project_v2() -> Project {
    let mut p = small_project();
    p.operations[0].tabs.manual = vec![ManualTabs {
        item: 1,
        positions: vec![0.125, 0.625],
    }];
    p
}

#[test]
fn m6_12_project_v2() {
    // v1 file migrates: version 2, every op has manual == []
    let (v1, problems) = Project::from_json(&fs::read_to_string(GOLDEN).unwrap()).unwrap();
    assert!(problems.is_empty());
    assert_eq!(v1.format_version, FORMAT_VERSION);
    assert_eq!(FORMAT_VERSION, 2);
    assert!(v1.operations.iter().all(|o| o.tabs.manual.is_empty()));
    assert_eq!(v1, small_project());

    // v2 golden: equal to the code-built project, and byte-exact to_json
    let text = fs::read_to_string(GOLDEN_V2).unwrap();
    let p = small_project_v2();
    let (loaded, problems) = Project::from_json(&text).unwrap();
    assert!(problems.is_empty());
    assert_eq!(loaded, p);
    assert_eq!(p.to_json().unwrap(), text);

    // too new, migrate identity at v2
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(migrate(v.clone(), 2).unwrap(), v);
    v["format_version"] = 3.into();
    assert!(matches!(
        Project::from_json(&v.to_string()),
        Err(ProjectError::VersionTooNew {
            found: 3,
            supported: 2
        })
    ));

    // library keeps version 1
    assert_eq!(LIBRARY_FORMAT_VERSION, 1);
    let dir = scratch("m6_12");
    let path = dir.join("tools.json");
    let mut lib = ToolLibrary::default();
    assert_eq!(lib.format_version, 1);
    lib.add(tool(3)).unwrap();
    lib.save(&path).unwrap();
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("\"format_version\": 1"));
    assert_eq!(ToolLibrary::load(&path).unwrap(), lib);

    // remove_item drops the manual entry
    let mut p = small_project_v2();
    p.remove_item(1);
    assert!(p.operations[0].tabs.manual.is_empty());
}
