//! M7 acceptance tests (specs/M7.md), items 1–12 (13 lives in hc-project, 14 in the app).
//! Machine safe 5 / clearance 2 / tool change 40, mm unless noted.
//! Golden files: tests/fixtures/gcode/<fixture><suffix>.ngc; `UPDATE_GOLDEN=1` rewrites them.

use hc_geom::{Circle, Contour, Point};
use hc_post::*;
use hc_project::{
    to_display, Direction, Geom, Item, OpKind, Project, Side, Source, TabPlacement, Tool, ToolKind,
    Units, WorkOrigin,
};
use hc_toolpath::{generate, Move, Point3, ToolpathError};
use std::fs;
use std::path::PathBuf;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn tool(id: u32, name: &str, d: f64) -> Tool {
    Tool {
        id,
        name: name.into(),
        kind: ToolKind::FlatEndMill,
        diameter: d,
        flute_length: 20.0,
        flutes: 2,
        feed: 800.0,
        plunge_feed: 200.0,
        rpm: 18000,
        depth_per_pass: 1.5,
        stepover_pct: 40.0,
        comment: String::new(),
    }
}

fn rect(x0: f64, y0: f64, w: f64, h: f64) -> Contour {
    let v = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)];
    Contour::from_bulge_vertices(&v.map(|(x, y)| (p(x, y), 0.0)), true)
}

fn circle(x: f64, y: f64, d: f64) -> Contour {
    Circle {
        center: p(x, y),
        radius: d / 2.0,
    }
    .to_contour()
}

fn add(pr: &mut Project, geom: Geom) -> u64 {
    let id = pr.next_id;
    pr.next_id += 1;
    pr.items.push(Item {
        id,
        layer: "0".into(),
        source: Source::Drawn,
        geom,
    });
    id
}

/// Adds an op and sets its cutting data: (depth, dpp, feed, plunge, rpm).
fn op(
    pr: &mut Project,
    name: &str,
    kind: OpKind,
    t: &Tool,
    items: Vec<u64>,
    (depth, dpp, feed, plunge, rpm): (f64, f64, f64, f64, u32),
) -> u64 {
    let id = pr.add_operation(name, kind, t, items);
    let o = pr.operations.last_mut().unwrap();
    o.depth = depth;
    o.depth_per_pass = dpp;
    o.feed = feed;
    o.plunge_feed = plunge;
    o.rpm = rpm;
    id
}

fn base() -> Project {
    let pr = Project::new();
    assert_eq!(
        (
            pr.machine.safe_z,
            pr.machine.clearance_z,
            pr.machine.tool_change_z
        ),
        (5.0, 2.0, 40.0)
    );
    pr
}

fn outside() -> OpKind {
    OpKind::Contour {
        side: Side::Outside,
        direction: Direction::Climb,
    }
}

/// Fixture 1.
fn example() -> Project {
    let mut pr = base();
    let sq = add(&mut pr, Geom::Contour(rect(0.0, 0.0, 20.0, 20.0)));
    let pt = add(&mut pr, Geom::Point(p(10.0, 10.0)));
    let t1 = tool(1, "Flat 3mm", 3.0);
    let t2 = Tool {
        kind: ToolKind::Drill,
        ..tool(2, "Drill 3.2 (HSS)", 3.2)
    };
    op(
        &mut pr,
        "Outline",
        OpKind::Engrave,
        &t1,
        vec![sq],
        (1.0, 1.0, 600.0, 150.0, 12000),
    );
    op(
        &mut pr,
        "Holes",
        OpKind::Drill { peck: Some(1.5) },
        &t2,
        vec![pt],
        (5.0, 1.0, 600.0, 100.0, 3000),
    );
    pr
}

/// Fixture 2.
fn square_outside() -> Project {
    let mut pr = base();
    let sq = add(&mut pr, Geom::Contour(rect(0.0, 0.0, 20.0, 20.0)));
    let t1 = tool(1, "Flat 6mm", 6.0);
    op(
        &mut pr,
        "Outside",
        outside(),
        &t1,
        vec![sq],
        (3.0, 1.5, 800.0, 200.0, 18000),
    );
    pr
}

/// Fixture 3 (fixture 4 = this with inch units).
fn pocket_drill() -> Project {
    let mut pr = base();
    let c = add(&mut pr, Geom::Contour(circle(50.0, 20.0, 30.0)));
    let h1 = add(&mut pr, Geom::Point(p(10.0, 10.0)));
    let h2 = add(&mut pr, Geom::Point(p(90.0, 10.0)));
    let t1 = tool(1, "Flat 6mm", 6.0);
    let t2 = Tool {
        kind: ToolKind::Drill,
        ..tool(2, "Drill 3mm", 3.0)
    };
    let data = (3.0, 1.5, 800.0, 200.0, 18000);
    op(
        &mut pr,
        "Pocket",
        OpKind::Pocket { stepover_pct: 40.0 },
        &t1,
        vec![c],
        data,
    );
    op(&mut pr, "Wall", outside(), &t1, vec![c], data);
    op(
        &mut pr,
        "Holes",
        OpKind::Drill { peck: None },
        &t2,
        vec![h1, h2],
        (6.0, 1.0, 800.0, 100.0, 3000),
    );
    pr
}

fn pocket_drill_inch() -> Project {
    let mut pr = pocket_drill();
    pr.units = Units::Inch;
    pr
}

/// Fixture 5.
fn tabbed_contour() -> Project {
    let mut pr = base();
    let r = add(&mut pr, Geom::Contour(rect(0.0, 0.0, 40.0, 20.0)));
    let t1 = tool(1, "Flat 3mm", 3.0);
    op(
        &mut pr,
        "Cut out",
        outside(),
        &t1,
        vec![r],
        (3.0, 1.5, 800.0, 200.0, 18000),
    );
    let tabs = &mut pr.operations[0].tabs;
    tabs.enabled = true;
    tabs.placement = TabPlacement::Count { count: 2 };
    tabs.width = 4.0;
    tabs.height = 1.0;
    pr
}

fn fixtures() -> Vec<(&'static str, Project)> {
    vec![
        ("example", example()),
        ("square_outside", square_outside()),
        ("pocket_drill", pocket_drill()),
        ("pocket_drill_inch", pocket_drill_inch()),
        ("tabbed_contour", tabbed_contour()),
    ]
}

fn golden_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/gcode/{file}.ngc"))
}

/// Exports and compares (or with UPDATE_GOLDEN writes) every file. Returns the files.
fn check_golden(fixture: &str, pr: &Project) -> Vec<GcodeFile> {
    let files = export(pr, fixture).unwrap();
    for f in &files {
        let path = golden_path(&format!("{fixture}{}", f.suffix));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            fs::write(&path, &f.text).unwrap();
        }
        let want = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert_eq!(f.text, want, "{path:?}");
    }
    files
}

/// Code lines without comments.
fn code(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| strip_comments(l).trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn strip_comments(l: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in l.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

const EXAMPLE_1: &str = "(example - file 1 of 2)
(Generated by HobbyCAM, LinuxCNC post)
(Units: mm. Z0 = top of stock. XY0 = drawing origin)
(Tool: T1 Flat 3mm)
(Op: Outline)
(Before running: load T1 Flat 3mm and set Z0 on top of stock)
G17 G21 G40 G49 G80 G90 G91.1 G94 G98
G64 P0.01
G0 Z5
S12000 M3
G4 P2
(Outline)
G0 X0 Y0 Z5
G0 X0 Y0 Z2
G1 X0 Y0 Z-1 F150
G1 X20 Y0 Z-1 F600
G1 X20 Y20 Z-1
G1 X0 Y20 Z-1
G1 X0 Y0 Z-1
G0 X0 Y0 Z5
G0 Z40
M5
M2
";

const EXAMPLE_2: &str = "(example - file 2 of 2)
(Generated by HobbyCAM, LinuxCNC post)
(Units: mm. Z0 = top of stock. XY0 = drawing origin)
(Tool: T2 Drill 3.2 [HSS])
(Op: Holes)
(Before running: load T2 Drill 3.2 [HSS] and set Z0 on top of stock)
G17 G21 G40 G49 G80 G90 G91.1 G94 G98
G64 P0.01
G0 Z5
S3000 M3
G4 P2
(Holes)
G0 X10 Y10 Z5
G83 X10 Y10 Z-5 R2 Q1.5 F100
G80
G0 Z40
M5
M2
";

#[test]
fn t01_example_exact() {
    // exact spec text first, independent of UPDATE_GOLDEN
    let files = export(&example(), "example").unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].suffix, "_1_T1");
    assert_eq!(files[1].suffix, "_2_T2");
    assert_eq!(files[0].text, EXAMPLE_1);
    assert_eq!(files[1].text, EXAMPLE_2);
    check_golden("example", &example());
    assert_eq!(LinuxCnc.extension(), "ngc");
}

#[test]
fn t02_square_outside() {
    let files = check_golden("square_outside", &square_outside());
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].suffix, "");
    let lines = code(&files[0].text);
    assert!(!lines.iter().any(|l| l.starts_with("G3")));
    let arcs: Vec<&String> = lines.iter().filter(|l| l.starts_with("G2")).collect();
    assert!(!arcs.is_empty());
    for a in &arcs {
        let i = word(a, 'I').unwrap();
        let j = word(a, 'J').unwrap();
        assert!(((i * i + j * j).sqrt() - 3.0).abs() < 2e-3, "{a}");
    }
    let mut zs: Vec<String> = lines
        .iter()
        .filter(|l| l.starts_with("G1 ") || l.starts_with("G2 "))
        .map(|l| fmt_num(word(l, 'Z').unwrap(), 3))
        .collect();
    zs.sort();
    zs.dedup();
    assert_eq!(zs, ["-1.5", "-3"]);
}

#[test]
fn t03_pocket_drill() {
    let files = check_golden("pocket_drill", &pocket_drill());
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].suffix, "_1_T1");
    assert_eq!(files[1].suffix, "_2_T2");
    let t0 = &files[0].text;
    assert_eq!(t0.matches("(Op: ").count(), 2);
    assert_eq!(t0.lines().filter(|l| l.ends_with(" M3")).count(), 1);
    assert_eq!(
        code(&files[1].text)
            .iter()
            .filter(|l| l.starts_with("G81"))
            .count(),
        2
    );
    for f in &files {
        assert_no_tool_change(&f.text);
    }
}

fn assert_no_tool_change(text: &str) {
    for l in code(text) {
        for w in l.split_whitespace() {
            assert!(!w.starts_with('T') && w != "M6" && w != "M0", "{l}");
        }
    }
}

#[test]
fn t04_pocket_drill_inch() {
    let files = check_golden("pocket_drill_inch", &pocket_drill_inch());
    assert_eq!(files.len(), 2);
    for f in &files {
        assert!(f.text.contains("\nG17 G20 G40 G49 G80 G90 G91.1 G94 G98\n"));
        assert!(f.text.contains("\nG64 P0.0004\n"));
        assert!(f.text.ends_with("\nG0 Z1.5748\nM5\nM2\n"));
        assert!(f.text.contains("(Units: inch."));
        for l in code(&f.text) {
            for w in l.split_whitespace().skip(1) {
                let frac = w[1..].split('.').nth(1).unwrap_or("");
                assert!(frac.len() <= 4, "{l}");
            }
        }
    }
}

#[test]
fn t05_tabbed_contour() {
    let files = check_golden("tabbed_contour", &tabbed_contour());
    let lines = code(&files[0].text);
    let xyz = |l: &str| (word(l, 'X'), word(l, 'Y'), word(l, 'Z'));
    let mut lifts = 0;
    for k in 1..lines.len() {
        let (a, b) = (&lines[k - 1], &lines[k]);
        let (ax, ay, az) = xyz(a);
        let (bx, by, bz) = xyz(b);
        if b.starts_with("G1 ") && (ax, ay) == (bx, by) && bz == Some(-2.0) {
            assert_eq!(az, Some(-3.0), "lift only from the Z-3 pass: {a} / {b}");
            // vertical up at the plunge feed, the tab run at the cutting feed
            assert!(b.ends_with(" F200"), "{b}");
            assert!(lines[k + 1].ends_with(" F800"), "{}", lines[k + 1]);
            lifts += 1;
        }
    }
    assert_eq!(lifts, 2);
    // the Z-1.5 pass never goes to Z-2 directly from Z-1.5
    for k in 1..lines.len() {
        if word(&lines[k - 1], 'Z') == Some(-1.5) {
            assert_ne!(word(&lines[k], 'Z'), Some(-2.0), "{}", lines[k]);
        }
    }
}

// ---------------------------------------------------------------- 6–11

#[test]
fn t06_fmt_num() {
    assert_eq!(fmt_num(-0.0004, 3), "0");
    assert_eq!(fmt_num(1e-12, 3), "0");
    assert_eq!(fmt_num(-0.0, 3), "0");
    assert_eq!(fmt_num(10.0, 3), "10");
    assert_eq!(fmt_num(-1.5, 3), "-1.5");
    assert_eq!(fmt_num(0.1 + 0.2, 3), "0.3");
    assert_eq!(fmt_num(1e15, 3), "1000000000000000");
    assert_eq!(fmt_num(0.00049, 4), "0.0005");
    assert_eq!(fmt_num(25.4 / 25.4, 4), "1");
}

fn p3(x: f64, y: f64, z: f64) -> Point3 {
    Point3 { x, y, z }
}

fn job(units: Units, moves: Vec<Move>) -> Job {
    Job {
        name: "t".into(),
        index: 1,
        count: 1,
        units,
        origin: WorkOrigin::DrawingOrigin,
        safe_z: 5.0,
        tool_change_z: 40.0,
        tool: 1,
        tool_name: "Flat".into(),
        ops: vec![JobOp {
            name: "op".into(),
            rpm: 10000,
            moves,
        }],
    }
}

/// The move lines of a one-op program (between "(op)" and the footer).
fn body(units: Units, moves: Vec<Move>) -> Vec<String> {
    let text = LinuxCnc.program(&job(units, moves));
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| *l == "(op)").unwrap() + 1;
    lines[start..lines.len() - 3]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn lin(x: f64, y: f64, z: f64, feed: f64) -> Move {
    Move::Linear {
        to: p3(x, y, z),
        feed,
    }
}

#[test]
fn t07_move_mapping() {
    let start = Move::Rapid(p3(0.0, 0.0, 5.0));
    assert_eq!(
        body(
            Units::Inch,
            vec![start.clone(), lin(25.4, 0.0, -1.27, 254.0)]
        )[1],
        "G1 X1 Y0 Z-0.05 F10"
    );
    // F modal: same feed twice → once; change → again; a Rapid in between → no F
    let b = body(
        Units::Mm,
        vec![
            start.clone(),
            lin(1.0, 0.0, -1.0, 600.0),
            lin(2.0, 0.0, -1.0, 600.0),
            lin(3.0, 0.0, -1.0, 500.0),
            Move::Rapid(p3(3.0, 0.0, 5.0)),
            lin(3.0, 0.0, -1.0, 500.0),
        ],
    );
    assert_eq!(
        b,
        [
            "G0 X0 Y0 Z5",
            "G1 X1 Y0 Z-1 F600",
            "G1 X2 Y0 Z-1",
            "G1 X3 Y0 Z-1 F500",
            "G0 X3 Y0 Z5",
            "G1 X3 Y0 Z-1",
        ]
    );
    // arcs: I/J relative to the current position
    let b = body(
        Units::Mm,
        vec![
            Move::Rapid(p3(10.0, 0.0, 5.0)),
            lin(10.0, 0.0, -1.0, 200.0),
            Move::Arc {
                to: p3(0.0, 10.0, -1.0),
                center: p(0.0, 0.0),
                ccw: true,
                feed: 600.0,
            },
            Move::Arc {
                to: p3(10.0, 0.0, -1.0),
                center: p(0.0, 0.0),
                ccw: false,
                feed: 600.0,
            },
        ],
    );
    assert_eq!(b[2], "G3 X0 Y10 Z-1 I-10 J0 F600");
    assert_eq!(b[3], "G2 X10 Y0 Z-1 I0 J-10");
    // tiny arc → G1
    let b = body(
        Units::Mm,
        vec![
            Move::Rapid(p3(0.0, 0.0, 5.0)),
            lin(0.0, 0.0, -1.0, 200.0),
            Move::Arc {
                to: p3(0.0002, 0.0, -1.0),
                center: p(0.0001, 0.5),
                ccw: true,
                feed: 200.0,
            },
        ],
    );
    assert_eq!(b[2], "G1 X0 Y0 Z-1");
    // drill cycles, then G80 before the next non-drill move and after the op's last drill
    let drill = |x: f64, peck| Move::Drill {
        at: p(x, 0.0),
        z_bottom: -4.0,
        peck,
        r_plane: 2.0,
        feed: 100.0,
    };
    let b = body(
        Units::Mm,
        vec![
            Move::Rapid(p3(0.0, 0.0, 5.0)),
            drill(0.0, None),
            Move::Rapid(p3(5.0, 0.0, 5.0)),
            drill(5.0, Some(2.0)),
        ],
    );
    assert_eq!(
        b,
        [
            "G0 X0 Y0 Z5",
            "G81 X0 Y0 Z-4 R2 F100",
            "G80",
            "G0 X5 Y0 Z5",
            "G83 X5 Y0 Z-4 R2 Q2",
            "G80",
        ]
    );
    // two drills back to back share one cycle block
    let b = body(
        Units::Mm,
        vec![
            Move::Rapid(p3(0.0, 0.0, 5.0)),
            drill(0.0, None),
            drill(5.0, None),
        ],
    );
    assert_eq!(b[1..], ["G81 X0 Y0 Z-4 R2 F100", "G81 X5 Y0 Z-4 R2", "G80"]);
}

/// Engrave op on a short line with tool `t` and rpm; returns the op id.
fn line_op(pr: &mut Project, item: u64, t: u32, rpm: u32) -> u64 {
    let tl = tool(t, &format!("Tool {t}"), 3.0);
    op(
        pr,
        &format!("op{}", pr.operations.len() + 1),
        OpKind::Engrave,
        &tl,
        vec![item],
        (1.0, 1.0, 600.0, 150.0, rpm),
    )
}

fn line_project() -> (Project, u64) {
    let mut pr = base();
    let id = add(
        &mut pr,
        Geom::Contour(Contour::from_bulge_vertices(
            &[(p(0.0, 0.0), 0.0), (p(10.0, 0.0), 0.0)],
            false,
        )),
    );
    (pr, id)
}

#[test]
fn t08_segments() {
    let (mut pr, it) = line_project();
    for t in [1, 1, 2, 1] {
        line_op(&mut pr, it, t, 10000);
    }
    let files = export(&pr, "s").unwrap();
    let suffixes: Vec<&str> = files.iter().map(|f| f.suffix.as_str()).collect();
    assert_eq!(suffixes, ["_1_T1", "_2_T2", "_3_T1"]);
    for (k, f) in files.iter().enumerate() {
        assert!(f.text.starts_with(&format!("(s - file {} of 3)\n", k + 1)));
    }
    assert_eq!(files[0].text.matches("(Op: ").count(), 2);

    // 12 alternating → zero-padded
    let (mut pr, it) = line_project();
    for k in 0..12 {
        line_op(&mut pr, it, 1 + k % 2, 10000);
    }
    let files = export(&pr, "s").unwrap();
    let suffixes: Vec<String> = files.iter().map(|f| f.suffix.clone()).collect();
    let want: Vec<String> = (1..=12)
        .map(|k| format!("_{k:02}_T{}", 2 - k % 2))
        .collect();
    assert_eq!(suffixes, want);
    assert!(files[11].text.starts_with("(s - file 12 of 12)\n"));

    // same tool, rpm change → one file, spindle restarted before op 2
    let (mut pr, it) = line_project();
    line_op(&mut pr, it, 1, 10000);
    line_op(&mut pr, it, 1, 12000);
    let files = export(&pr, "s").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].suffix, "");
    let t = &files[0].text;
    assert!(t.contains("G0 Z5\nS10000 M3\nG4 P2\n(op1)\n"));
    assert!(t.contains("(op2)\nS12000 M3\nG4 P2\n"));
    assert_eq!(t.matches("M3").count(), 2);

    // disabled op in between does not split
    let (mut pr, it) = line_project();
    line_op(&mut pr, it, 1, 10000);
    line_op(&mut pr, it, 2, 10000);
    line_op(&mut pr, it, 1, 10000);
    pr.operations[1].enabled = false;
    let files = export(&pr, "s").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].text.matches("(Op: ").count(), 2);
}

#[test]
fn t09_sanitize() {
    assert_eq!(sanitize_comment("Slot (left) Ø6\tmm"), "Slot [left] ?6 mm");
    assert_eq!(sanitize_comment(&"a".repeat(100)), "a".repeat(60));
    assert_eq!(sanitize_comment(""), "-");
    // used in headers
    let mut pr = example();
    pr.operations[0].name = "Slot (left) Ø6\tmm".into();
    let files = export(&pr, "a(b)").unwrap();
    assert!(files[0].text.starts_with("(a[b] - file 1 of 2)\n"));
    assert!(files[0].text.contains("\n(Op: Slot [left] ?6 mm)\n"));
    assert!(files[0].text.contains("\n(Slot [left] ?6 mm)\n"));
}

#[test]
fn t10_errors() {
    let mut pr = example();
    pr.machine.post = "grbl".into();
    assert_eq!(export(&pr, "x"), Err(PostError::UnknownPost("grbl".into())));
    // unknown post wins even for an empty project
    let mut empty = base();
    empty.machine.post = "grbl".into();
    assert_eq!(
        export(&empty, "x"),
        Err(PostError::UnknownPost("grbl".into()))
    );
    pr.machine.post = " LinuxCNC ".into();
    assert!(export(&pr, "x").is_ok());

    assert_eq!(export(&base(), "x"), Err(PostError::NothingToExport));
    let mut off = example();
    for o in &mut off.operations {
        o.enabled = false;
    }
    assert_eq!(export(&off, "x"), Err(PostError::NothingToExport));

    // too large tool for an inside contour
    let mut bad = example();
    let sq = bad.items[0].id;
    let big = tool(3, "Huge", 50.0);
    op(
        &mut bad,
        "Inside",
        OpKind::Contour {
            side: Side::Inside,
            direction: Direction::Climb,
        },
        &big,
        vec![sq],
        (1.0, 1.0, 600.0, 150.0, 10000),
    );
    let e = export(&bad, "x").unwrap_err();
    assert_eq!(
        e,
        PostError::OpFailed {
            op: "Inside".into(),
            error: ToolpathError::OffsetVanished(sq)
        }
    );
    assert_eq!(
        e.to_string(),
        format!("Operation \"Inside\" has an error: Tool too large for item {sq}")
    );
    bad.operations[2].enabled = false;
    assert!(export(&bad, "x").is_ok());

    let mut tc = example();
    tc.machine.tool_change_z = 4.0;
    assert_eq!(export(&tc, "x"), Err(PostError::ToolChangeBelowSafe));
    tc.machine.tool_change_z = f64::NAN;
    assert_eq!(export(&tc, "x"), Err(PostError::ToolChangeBelowSafe));
    tc.machine.tool_change_z = 5.0;
    let files = export(&tc, "x").unwrap();
    assert!(files[0].text.ends_with("G0 Z5\nM5\nM2\n"));
    assert_eq!(
        PostError::ToolChangeBelowSafe.to_string(),
        "Tool change Z must be at or above safe Z"
    );
    assert_eq!(
        PostError::NothingToExport.to_string(),
        "Nothing to export: no enabled operations"
    );
}

#[test]
fn t11_work_origin() {
    let mut pr = example();
    assert_eq!(work_origin(&pr), p(0.0, 0.0));
    pr.stock.origin = WorkOrigin::StockBottomLeft;
    pr.stock.min_x = -10.0;
    pr.stock.min_y = -5.0;
    assert_eq!(work_origin(&pr), p(-10.0, -5.0));
    let files = export(&pr, "x").unwrap();
    assert!(files[0]
        .text
        .contains("(Units: mm. Z0 = top of stock. XY0 = stock bottom left)\n"));
    assert_eq!(code(&files[0].text)[5], "G0 X10 Y5 Z5");

    let mut pr = example();
    pr.stock.origin = WorkOrigin::StockCenter;
    pr.stock.min_x = 0.0;
    pr.stock.min_y = 0.0;
    pr.stock.width = 100.0;
    pr.stock.height = 100.0;
    assert_eq!(work_origin(&pr), p(50.0, 50.0));
    let files = export(&pr, "x").unwrap();
    assert!(files[0].text.contains("XY0 = stock centre)\n"));
    assert_eq!(code(&files[0].text)[5], "G0 X-50 Y-50 Z5");

    // arc I/J do not change with the shift
    let mut a = square_outside();
    let ij = |pr: &Project| -> Vec<(String, String)> {
        let f = export(pr, "x").unwrap();
        code(&f[0].text)
            .iter()
            .filter(|l| l.starts_with("G2"))
            .map(|l| {
                let w: Vec<&str> = l.split_whitespace().collect();
                (w[4].to_string(), w[5].to_string())
            })
            .collect()
    };
    let before = ij(&a);
    a.stock.origin = WorkOrigin::StockBottomLeft;
    a.stock.min_x = -10.0;
    a.stock.min_y = -5.0;
    assert_eq!(ij(&a), before);
}

// ---------------------------------------------------------------- 12: round trip

/// Value of the word `letter` in a code line.
fn word(line: &str, letter: char) -> Option<f64> {
    line.split_whitespace()
        .find(|w| w.starts_with(letter))
        .map(|w| w[1..].parse().unwrap())
}

#[derive(Debug)]
enum Motion {
    /// G0 / G1 end point, feed in effect for G1.
    Straight {
        rapid: bool,
        to: [f64; 3],
        feed: Option<f64>,
    },
    Arc {
        from: [f64; 3],
        to: [f64; 3],
        ij: [f64; 2],
        ccw: bool,
        feed: Option<f64>,
    },
    Cycle {
        at: [f64; 2],
        z: f64,
        r: f64,
        q: Option<f64>,
        feed: Option<f64>,
    },
}

/// Test-only reader: G0/G1/G2/G3/G81/G83/G80, words X Y Z I J R Q F, comments stripped.
/// F is modal within one file (reset per file); positions start unknown.
fn read_gcode(text: &str) -> Vec<Motion> {
    let mut out = Vec::new();
    let mut pos = [f64::NAN; 3];
    let mut feed: Option<f64> = None;
    for l in code(text) {
        let words: Vec<&str> = l.split_whitespace().collect();
        for w in &words[1..] {
            let v = &w[1..];
            assert!(!v.contains(['e', 'E']), "exponent in {l}");
            assert_ne!(v, "-0", "{l}");
            assert!(!v.contains("NaN") && !v.contains("inf"), "{l}");
        }
        if let Some(f) = word(&l, 'F') {
            feed = Some(f);
        }
        let to = |pos: [f64; 3]| {
            [
                word(&l, 'X').unwrap_or(pos[0]),
                word(&l, 'Y').unwrap_or(pos[1]),
                word(&l, 'Z').unwrap_or(pos[2]),
            ]
        };
        match words[0] {
            // setup "G0 Z<safe>" / footer "G0 Z<tc>": not toolpath moves
            "G0" if word(&l, 'X').is_none() => pos = to(pos),
            g @ ("G0" | "G1") => {
                let t = to(pos);
                out.push(Motion::Straight {
                    rapid: g == "G0",
                    to: t,
                    feed: (g == "G1").then_some(feed).flatten(),
                });
                pos = t;
            }
            g @ ("G2" | "G3") => {
                let t = to(pos);
                out.push(Motion::Arc {
                    from: pos,
                    to: t,
                    ij: [word(&l, 'I').unwrap(), word(&l, 'J').unwrap()],
                    ccw: g == "G3",
                    feed,
                });
                pos = t;
            }
            "G81" | "G83" => {
                out.push(Motion::Cycle {
                    at: [word(&l, 'X').unwrap(), word(&l, 'Y').unwrap()],
                    z: word(&l, 'Z').unwrap(),
                    r: word(&l, 'R').unwrap(),
                    q: word(&l, 'Q'),
                    feed,
                });
                // G98: back to the Z before the cycle
                pos = [word(&l, 'X').unwrap(), word(&l, 'Y').unwrap(), pos[2]];
            }
            _ => {}
        }
    }
    out
}

#[test]
fn t12_round_trip() {
    for (name, pr) in fixtures() {
        let files = export(&pr, name).unwrap();
        let u = pr.units;
        let d = if u == Units::Inch { 4 } else { 3 };
        let tol = 0.5 * 10f64.powi(-d) + 1e-9;
        let tol_c = 2.0 * 10f64.powi(-d);
        let tc = fmt_num(to_display(pr.machine.tool_change_z, u), d as usize);
        let mut got = Vec::new();
        for f in &files {
            assert!(
                f.text.ends_with(&format!("G0 Z{tc}\nM5\nM2\n")),
                "{name}{}",
                f.suffix
            );
            assert_no_tool_change(&f.text);
            got.extend(read_gcode(&f.text));
        }
        let o = work_origin(&pr);
        let dx = |x: f64| to_display(x - o.x, u);
        let dy = |y: f64| to_display(y - o.y, u);
        let dz = |z: f64| to_display(z, u);
        let close = |a: f64, b: f64, t: f64| (a - b).abs() <= t;
        let close3 = |a: [f64; 3], b: Point3| {
            close(a[0], dx(b.x), tol) && close(a[1], dy(b.y), tol) && close(a[2], dz(b.z), tol)
        };
        let want: Vec<Move> = generate(&pr)
            .into_iter()
            .flat_map(|(_, r)| r.unwrap().moves)
            .collect();
        assert_eq!(got.len(), want.len(), "{name}");
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            let ctx = format!("{name} move {k}: {g:?} vs {w:?}");
            match (g, w) {
                (
                    Motion::Straight {
                        rapid: true, to, ..
                    },
                    Move::Rapid(p),
                ) => assert!(close3(*to, *p), "{ctx}"),
                (
                    Motion::Straight {
                        rapid: false,
                        to,
                        feed,
                    },
                    Move::Linear { to: p, feed: f },
                ) => {
                    assert!(close3(*to, *p), "{ctx}");
                    assert!(close(feed.expect(&ctx), dz(*f), tol), "{ctx}");
                }
                (
                    Motion::Arc {
                        from,
                        to,
                        ij,
                        ccw,
                        feed,
                    },
                    Move::Arc {
                        to: p,
                        center,
                        ccw: c,
                        feed: f,
                    },
                ) => {
                    assert!(close3(*to, *p), "{ctx}");
                    assert_eq!(ccw, c, "{ctx}");
                    assert!(close(from[0] + ij[0], dx(center.x), tol_c), "{ctx}");
                    assert!(close(from[1] + ij[1], dy(center.y), tol_c), "{ctx}");
                    // LinuxCNC centre-format check on the written values (M7 review ruling 1)
                    let r1 = ij[0].hypot(ij[1]);
                    let r2 = (from[0] + ij[0] - to[0]).hypot(from[1] + ij[1] - to[1]);
                    let min_r = 10.0 * 10f64.powi(-d) - 1e-12;
                    assert!(r1 >= min_r && r2 >= min_r, "{ctx}: r1 {r1} r2 {r2}");
                    assert!((r1 - r2).abs() <= 5.0 * 10f64.powi(-d) + 1e-12, "{ctx}");
                    assert!(close(feed.expect(&ctx), dz(*f), tol), "{ctx}");
                }
                // tiny arc written as G1
                (
                    Motion::Straight {
                        rapid: false,
                        to,
                        feed,
                    },
                    Move::Arc { to: p, feed: f, .. },
                ) => {
                    assert!(close3(*to, *p), "{ctx}");
                    assert!(close(feed.expect(&ctx), dz(*f), tol), "{ctx}");
                }
                (
                    Motion::Cycle { at, z, r, q, feed },
                    Move::Drill {
                        at: a,
                        z_bottom,
                        peck,
                        r_plane,
                        feed: f,
                    },
                ) => {
                    assert!(
                        close(at[0], dx(a.x), tol) && close(at[1], dy(a.y), tol),
                        "{ctx}"
                    );
                    assert!(close(*z, dz(*z_bottom), tol), "{ctx}");
                    assert!(close(*r, dz(*r_plane), tol), "{ctx}");
                    match (q, peck) {
                        (None, None) => {}
                        (Some(q), Some(pk)) => assert!(close(*q, dz(*pk), tol), "{ctx}"),
                        _ => panic!("{ctx}"),
                    }
                    assert!(close(feed.expect(&ctx), dz(*f), tol), "{ctx}");
                }
                _ => panic!("mismatch {ctx}"),
            }
        }
    }
}

/// The reader itself: F does not leak across files, arcs keep their start.
#[test]
fn reader_self_check() {
    let m = read_gcode("(c) G0 X1 Y2 Z3\nG1 X2 Y2 Z3 F10\nG2 X3 Y3 Z3 I1 J0\nG81 X0 Y0 Z-1 R2\n");
    assert_eq!(m.len(), 4);
    assert!(matches!(
        m[0],
        Motion::Straight {
            rapid: true,
            feed: None,
            ..
        }
    ));
    assert!(matches!(
        m[2],
        Motion::Arc {
            from: [2.0, 2.0, 3.0],
            ccw: false,
            feed: Some(10.0),
            ..
        }
    ));
    assert!(matches!(
        m[3],
        Motion::Cycle {
            q: None,
            feed: Some(10.0),
            ..
        }
    ));
    assert!(matches!(
        read_gcode("G1 X0 Y0 Z0")[0],
        Motion::Straight { feed: None, .. }
    ));
}
