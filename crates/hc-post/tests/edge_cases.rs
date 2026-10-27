//! M7 edge cases (tester): number formatting, arcs, segments, naming, comments, work origin,
//! tool change Z, F modality, a structural check of every golden file, v2 → v3 migrate.

use hc_geom::{Circle, Contour, Point};
use hc_post::*;
use hc_project::{
    Direction, Geom, Item, OpKind, Project, ProjectError, Side, Source, TabPlacement, Tool,
    ToolKind, Units, WorkOrigin,
};
use hc_toolpath::{generate, Move, Point3, RuleId};
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------- helpers

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn p3(x: f64, y: f64, z: f64) -> Point3 {
    Point3 { x, y, z }
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

fn drill_tool(id: u32, d: f64) -> Tool {
    Tool {
        kind: ToolKind::Drill,
        ..tool(id, &format!("Drill {d}"), d)
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

fn op(pr: &mut Project, name: &str, kind: OpKind, t: &Tool, items: Vec<u64>) -> u64 {
    let id = pr.add_operation(name, kind, t, items);
    let o = pr.operations.last_mut().unwrap();
    o.depth = 3.0;
    o.depth_per_pass = 1.5;
    o.feed = 800.0;
    o.plunge_feed = 200.0;
    o.rpm = 18000;
    id
}

fn outside(direction: Direction) -> OpKind {
    OpKind::Contour {
        side: Side::Outside,
        direction,
    }
}

/// Short open line + n engrave ops with the given tools.
fn line_ops(tools: &[u32]) -> Project {
    let mut pr = Project::new();
    let it = add(
        &mut pr,
        Geom::Contour(Contour::from_bulge_vertices(
            &[(p(0.0, 0.0), 0.0), (p(10.0, 0.0), 0.0)],
            false,
        )),
    );
    for (k, &t) in tools.iter().enumerate() {
        let tl = tool(t, &format!("Tool {t}"), 3.0);
        op(
            &mut pr,
            &format!("op{}", k + 1),
            OpKind::Engrave,
            &tl,
            vec![it],
        );
    }
    pr
}

/// Mixed project: pocket + outside contour (T1), drill (T2).
fn mixed() -> Project {
    let mut pr = Project::new();
    let c = add(&mut pr, Geom::Contour(circle(50.0, 20.0, 30.0)));
    let r = add(&mut pr, Geom::Contour(rect(0.0, 0.0, 20.0, 10.0)));
    let h = add(&mut pr, Geom::Point(p(10.0, 30.0)));
    let t1 = tool(1, "Flat 6mm", 6.0);
    op(
        &mut pr,
        "Pocket",
        OpKind::Pocket { stepover_pct: 40.0 },
        &t1,
        vec![c],
    );
    op(&mut pr, "Outer", outside(Direction::Climb), &t1, vec![r]);
    op(
        &mut pr,
        "Holes",
        OpKind::Drill { peck: Some(1.0) },
        &drill_tool(2, 3.0),
        vec![h],
    );
    pr
}

fn job(units: Units, ops: Vec<Vec<Move>>) -> Job {
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
        ops: ops
            .into_iter()
            .enumerate()
            .map(|(k, moves)| JobOp {
                name: format!("op{}", k + 1),
                rpm: 10000,
                moves,
            })
            .collect(),
    }
}

/// Non-comment, non-empty lines.
fn code(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('('))
        .map(|l| l.to_string())
        .collect()
}

fn word(line: &str, letter: char) -> Option<f64> {
    line.split_whitespace()
        .skip(1)
        .find(|w| w.starts_with(letter))
        .map(|w| w[1..].parse().unwrap())
}

fn lin(x: f64, y: f64, z: f64, feed: f64) -> Move {
    Move::Linear {
        to: p3(x, y, z),
        feed,
    }
}

fn arc(to: Point3, c: Point, ccw: bool) -> Move {
    Move::Arc {
        to,
        center: c,
        ccw,
        feed: 600.0,
    }
}

/// Code lines of a LinuxCNC program between the setup and the footer.
fn moves_of(text: &str) -> Vec<String> {
    let c = code(text);
    // setup: G17…, G64, G0 Z, S M3, G4 ; footer: G0 Z, M5, M2
    c[5..c.len() - 3].to_vec()
}

// ---------------------------------------------------------------- number formatting

#[test]
fn fmt_num_edges() {
    assert_eq!(fmt_num(-0.0004, 3), "0");
    assert_eq!(fmt_num(-1e-7, 3), "0");
    assert_eq!(fmt_num(-1e-7, 4), "0");
    assert_eq!(fmt_num(1e-12, 4), "0");
    assert_eq!(fmt_num(1e-300, 3), "0");
    // 0.0005 as f64 is slightly above the half → rounds up (not a tie)
    assert_eq!(fmt_num(0.0005, 3), "0.001");
    assert_eq!(fmt_num(-0.0005, 3), "-0.001");
    assert_eq!(fmt_num(0.0004999, 3), "0");
    assert_eq!(fmt_num(123456.7891, 3), "123456.789");
    assert_eq!(fmt_num(123456.7891, 4), "123456.7891");
    assert_eq!(fmt_num(2.9999, 3), "3");
    assert_eq!(fmt_num(-2.9999, 3), "-3");
    assert_eq!(fmt_num(9.99996, 4), "10");
    assert_eq!(fmt_num(100.0, 3), "100"); // zeros before the point are kept
    assert_eq!(fmt_num(-100.0, 3), "-100");
    assert_eq!(fmt_num(0.5, 3), "0.5");
    assert_eq!(fmt_num(-0.05, 4), "-0.05");
    assert_eq!(fmt_num(1.2340, 4), "1.234");
    assert_eq!(fmt_num(1e20, 3), "100000000000000000000");
    assert_eq!(fmt_num(-1e-20, 0), "0");
    assert_eq!(fmt_num(7.0, 0), "7");
    for v in [1e-9, 1e21, -1e21, 1e300, f64::MAX, f64::MIN_POSITIVE] {
        let s = fmt_num(v, 4);
        assert!(!s.contains(['e', 'E']), "{v} → {s}");
        assert!(!s.ends_with('.') && s != "-0", "{v} → {s}");
        if s.contains('.') {
            assert!(!s.ends_with('0'), "{v} → {s}");
        }
    }
}

#[test]
fn inch_conversion_by_hand() {
    // 25.4 mm → 1 in; 52.4 mm → 2.06299… → 2.063; 5 mm → 0.19685 → 0.1969; 800 mm/min → 31.4961
    let t = LinuxCnc.program(&job(
        Units::Inch,
        vec![vec![
            Move::Rapid(p3(25.4, 52.4, 5.0)),
            lin(25.4, 52.4, -1.27, 800.0),
            lin(-25.4, 0.0, -0.0001, 800.0004),
        ]],
    ));
    assert_eq!(
        moves_of(&t),
        [
            "G0 X1 Y2.063 Z0.1969",
            "G1 X1 Y2.063 Z-0.05 F31.4961",
            // -0.0001 mm = -0.0000039 in → 0; 800.0004 mm/min still formats as F31.4961 → no F
            "G1 X-1 Y0 Z0",
        ]
    );
    assert!(t.contains("\nG0 Z0.1969\n"));
    assert!(t.ends_with("\nG0 Z1.5748\nM5\nM2\n"));
}

// ---------------------------------------------------------------- arcs

#[test]
fn inch_arc_ij_by_hand() {
    // (10,0) → (0,10) about (0,0): I = -10 mm = -0.3937 in, J = 0
    let t = LinuxCnc.program(&job(
        Units::Inch,
        vec![vec![
            Move::Rapid(p3(10.0, 0.0, 5.0)),
            lin(10.0, 0.0, -1.0, 200.0),
            arc(p3(0.0, 10.0, -1.0), p(0.0, 0.0), true),
        ]],
    ));
    assert_eq!(
        moves_of(&t)[2],
        "G3 X0 Y0.3937 Z-0.0394 I-0.3937 J0 F23.622"
    );
}

#[test]
fn arc_ij_uses_unrounded_current_position() {
    // start (1.0004, 0) → I = 0 - 1.0004 = -1.0004 → "-1" (formatted from the unrounded difference)
    let b = moves_of(&LinuxCnc.program(&job(
        Units::Mm,
        vec![vec![
            Move::Rapid(p3(1.0004, 0.0, 5.0)),
            lin(1.0004, 0.0, -1.0, 200.0),
            arc(p3(-1.0004, 0.0, -1.0), p(0.0, 0.0), true),
            arc(p3(0.0, 1.0004, -1.0), p(0.0, 0.0), false),
        ]],
    )));
    assert_eq!(b[2], "G3 X-1 Y0 Z-1 I-1 J0 F600");
    // second arc starts at the tracked (unrounded) end of the first: I = 1.0004 → "1"
    assert_eq!(b[3], "G2 X0 Y1 Z-1 I1 J0");
}

#[test]
fn tiny_and_degenerate_arcs() {
    let start = vec![Move::Rapid(p3(0.0, 0.0, 5.0)), lin(0.0, 0.0, -1.0, 600.0)];
    let body = |units, a: Move| {
        let mut m = start.clone();
        m.push(a);
        moves_of(&LinuxCnc.program(&job(units, vec![m])))[2].clone()
    };
    // radius 0.001 mm, endpoints 0.002 apart → G1 (M7 review ruling 1: r < 0.01 mm; was G3)
    assert_eq!(
        body(Units::Mm, arc(p3(0.002, 0.0, -1.0), p(0.001, 0.0), true)),
        "G1 X0.002 Y0 Z-1"
    );
    // endpoints round to the same mm value → G1
    assert_eq!(
        body(Units::Mm, arc(p3(0.0004, 0.0, -1.0), p(0.0002, 0.0), false)),
        "G1 X0 Y0 Z-1"
    );
    // same unrounded endpoints (would be a full circle in LinuxCNC) → G1
    assert_eq!(
        body(Units::Mm, arc(p3(0.0, 0.0, -1.0), p(5.0, 0.0), true)),
        "G1 X0 Y0 Z-1"
    );
    // inch: end 0.002 mm = 0.0000787 in → "0.0001" differs from the start "0", but the centre formats to
    // I0 J0 (zero radius) → G1 (M7 review ruling 1; was "G3 … I0 J0")
    assert!(body(Units::Mm, arc(p3(0.002, 0.0, -1.0), p(0.001, 0.0), true)).starts_with("G1"));
    assert_eq!(
        body(Units::Inch, arc(p3(0.002, 0.0, -1.0), p(0.001, 0.0), true)),
        "G1 X0.0001 Y0 Z-0.0394"
    );
    // ends differ only in Y after rounding, r 0.0005 mm → G1 (M7 review ruling 1; was G3)
    assert!(body(Units::Mm, arc(p3(0.0, 0.001, -1.0), p(0.0, 0.0005), true)).starts_with("G1"));
    // quarter arcs: r 0.004 mm → G1, r 0.02 mm → G2 / G3
    assert_eq!(
        body(Units::Mm, arc(p3(0.004, 0.004, -1.0), p(0.0, 0.004), true)),
        "G1 X0.004 Y0.004 Z-1"
    );
    assert_eq!(
        body(Units::Mm, arc(p3(0.02, 0.02, -1.0), p(0.0, 0.02), true)),
        "G3 X0.02 Y0.02 Z-1 I0 J0.02"
    );
    assert_eq!(
        body(Units::Mm, arc(p3(0.02, 0.02, -1.0), p(0.02, 0.0), false)),
        "G2 X0.02 Y0.02 Z-1 I0.02 J0"
    );
}

#[test]
fn tiny_arc_g1_keeps_feed_modal() {
    let b = moves_of(&LinuxCnc.program(&job(
        Units::Mm,
        vec![vec![
            Move::Rapid(p3(0.0, 0.0, 5.0)),
            lin(0.0, 0.0, -1.0, 200.0),
            Move::Arc {
                to: p3(0.0001, 0.0, -1.0),
                center: p(0.00005, 0.0),
                ccw: true,
                feed: 700.0,
            },
            lin(5.0, 0.0, -1.0, 700.0),
        ]],
    )));
    assert_eq!(b[2], "G1 X0 Y0 Z-1 F700");
    assert_eq!(b[3], "G1 X5 Y0 Z-1");
}

// ---------------------------------------------------------------- structural checker

#[derive(Default)]
struct Stats {
    arcs: usize,
    g2: usize,
    g3: usize,
    cycles: usize,
}

/// Checks one program: format rules, continuity, arc radius consistency, F in effect for feed moves,
/// no rapid XY move below the stock top, no feed move above Z0 that is not a plunge/retract, cycle closing.
fn check_program(text: &str, units: Units, tc: f64) -> Stats {
    let d = if units == Units::Inch { 4 } else { 3 };
    let q = 10f64.powi(-d);
    assert!(text.ends_with('\n') && !text.contains('\r'));
    assert!(
        text.ends_with(&format!("\nG0 Z{}\nM5\nM2\n", fmt_num(tc, d as usize))),
        "{text}"
    );
    let mut st = Stats::default();
    let mut pos: Option<[f64; 3]> = None;
    let mut f: Option<f64> = None;
    let mut in_cycle = false;
    for l in text.lines() {
        assert_eq!(l, l.trim_end(), "trailing space");
        assert!(!l.is_empty());
        if l.starts_with('(') {
            assert!(l.ends_with(')') && l[1..l.len() - 1].chars().all(|c| c != '(' && c != ')'));
            assert!(l.is_ascii());
            continue;
        }
        let ws: Vec<&str> = l.split_whitespace().collect();
        for w in &ws {
            assert!(!w.contains(['e', 'E']) && !w.contains("NaN") && !w.contains("inf"));
            let (letter, v) = w.split_at(1);
            assert_ne!(letter, "T", "{l}");
            assert!(*w != "M6" && *w != "M0" && *w != "G43", "{l}");
            if ["X", "Y", "Z", "I", "J", "R", "Q", "F"].contains(&letter) {
                assert_ne!(v, "-0", "{l}");
                assert_eq!(fmt_num(v.parse().unwrap(), d as usize), v, "{l}");
            }
        }
        let g = ws[0];
        if let Some(fw) = word(l, 'F') {
            f = Some(fw);
        }
        if in_cycle && !matches!(g, "G81" | "G83" | "G80") {
            panic!("motion inside an open canned cycle: {l}");
        }
        match g {
            "G0" if word(l, 'X').is_none() => {
                // setup / footer Z only line
                let z = word(l, 'Z').unwrap();
                pos = pos.map(|p| [p[0], p[1], z]);
            }
            "G0" | "G1" | "G2" | "G3" => {
                let to = [
                    word(l, 'X').expect(l),
                    word(l, 'Y').expect(l),
                    word(l, 'Z').expect(l),
                ];
                if g == "G0" {
                    if let Some(p) = pos {
                        if (p[0], p[1]) != (to[0], to[1]) {
                            assert!(p[2] > 0.0 && to[2] > 0.0, "rapid XY move at/below Z0: {l}");
                        }
                    }
                    assert!(
                        to[2] > 0.0,
                        "rapid down into the stock (spec allows ≥ 0?): {l}"
                    );
                } else {
                    assert!(f.is_some(), "feed move without F in this file: {l}");
                    let from = pos.expect("feed move before any position");
                    if from[2] > 0.0 || to[2] > 0.0 {
                        assert!(
                            (from[0], from[1]) == (to[0], to[1]) && g == "G1",
                            "cutting move above Z0 that is not vertical: {l}"
                        );
                    }
                }
                if g == "G2" || g == "G3" {
                    let from = pos.unwrap();
                    let (i, j) = (word(l, 'I').unwrap(), word(l, 'J').unwrap());
                    let c = [from[0] + i, from[1] + j];
                    let r0 = i.hypot(j);
                    let r1 = (to[0] - c[0]).hypot(to[1] - c[1]);
                    // rounding of start, end and I/J: each ≤ q/2 per axis
                    assert!(
                        (r0 - r1).abs() <= 3.0 * q,
                        "radius mismatch {r0} vs {r1}: {l}"
                    );
                    assert!((from[0], from[1]) != (to[0], to[1]), "full circle: {l}");
                    assert_eq!(from[2], to[2], "helix not expected: {l}");
                    st.arcs += 1;
                    if g == "G2" {
                        st.g2 += 1
                    } else {
                        st.g3 += 1
                    }
                }
                pos = Some(to);
            }
            "G81" | "G83" => {
                assert!(f.is_some(), "{l}");
                assert_eq!(g == "G83", word(l, 'Q').is_some(), "{l}");
                let p = pos.expect("cycle before any position");
                let r = word(l, 'R').unwrap();
                assert!(r < p[2] + 1e-9 && word(l, 'Z').unwrap() < r, "{l}");
                pos = Some([word(l, 'X').unwrap(), word(l, 'Y').unwrap(), p[2]]);
                in_cycle = true;
                st.cycles += 1;
            }
            "G80" => {
                assert!(in_cycle, "G80 without a cycle");
                in_cycle = false;
            }
            _ => {}
        }
    }
    assert!(!in_cycle, "cycle not closed by G80");
    st
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/gcode")
}

#[test]
fn every_golden_file_is_well_formed() {
    let mut n = 0;
    let mut total = Stats::default();
    for e in fs::read_dir(golden_dir()).unwrap() {
        let path = e.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) != Some("ngc") {
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.is_ascii(), "{path:?}");
        let text = String::from_utf8(bytes).unwrap();
        let inch = text.contains("\nG20 ") || text.contains(" G20 ");
        let units = if inch { Units::Inch } else { Units::Mm };
        let st = check_program(&text, units, if inch { 40.0 / 25.4 } else { 40.0 });
        // header: first line "(<name> - file k of N)", Before running line present
        let first = text.lines().next().unwrap();
        assert!(
            first.contains(" - file ") && first.contains(" of "),
            "{path:?}"
        );
        assert!(text.contains("\n(Before running: load T"), "{path:?}");
        total.arcs += st.arcs;
        total.g2 += st.g2;
        total.g3 += st.g3;
        total.cycles += st.cycles;
        n += 1;
    }
    assert_eq!(n, 8);
    assert!(total.arcs > 0 && total.g2 > 0 && total.g3 > 0 && total.cycles > 0);
}

#[test]
fn generated_variants_are_well_formed() {
    // mixed project in all units × origins, plus tabbed circles / rectangles, conventional and climb
    let mut projects = Vec::new();
    for units in [Units::Mm, Units::Inch] {
        for origin in [
            WorkOrigin::DrawingOrigin,
            WorkOrigin::StockBottomLeft,
            WorkOrigin::StockCenter,
        ] {
            let mut pr = mixed();
            pr.units = units;
            pr.stock.origin = origin;
            pr.stock.min_x = -7.3;
            pr.stock.min_y = 3.1;
            projects.push(pr);
        }
    }
    for dir in [Direction::Climb, Direction::Conventional] {
        for side in [Side::Outside, Side::Inside] {
            let mut pr = Project::new();
            let c = add(&mut pr, Geom::Contour(circle(0.0, 0.0, 40.0)));
            let r = add(&mut pr, Geom::Contour(rect(30.0, 0.0, 40.0, 20.0)));
            let t = tool(1, "Flat 3mm", 3.0);
            op(
                &mut pr,
                "Tabbed",
                OpKind::Contour {
                    side,
                    direction: dir,
                },
                &t,
                vec![c, r],
            );
            let tabs = &mut pr.operations[0].tabs;
            tabs.enabled = true;
            tabs.placement = TabPlacement::Count { count: 3 };
            tabs.width = 4.0;
            tabs.height = 1.0;
            projects.push(pr);
        }
    }
    for pr in projects {
        let files = export(&pr, "v").unwrap();
        let tc = hc_project::to_display(pr.machine.tool_change_z, pr.units);
        for f in &files {
            check_program(&f.text, pr.units, tc);
        }
    }
}

#[test]
fn tab_split_arcs_keep_direction() {
    // circle with tabs: every arc piece of the split keeps the op's direction; G2/G3 matches generate()
    for (dir, want_ccw) in [(Direction::Climb, false), (Direction::Conventional, true)] {
        let mut pr = Project::new();
        let c = add(&mut pr, Geom::Contour(circle(0.0, 0.0, 40.0)));
        op(
            &mut pr,
            "C",
            outside(dir),
            &tool(1, "Flat 3mm", 3.0),
            vec![c],
        );
        let tabs = &mut pr.operations[0].tabs;
        tabs.enabled = true;
        tabs.placement = TabPlacement::Count { count: 3 };
        tabs.width = 4.0;
        tabs.height = 1.0;
        let ccws: Vec<bool> = generate(&pr)[0]
            .1
            .as_ref()
            .unwrap()
            .moves
            .iter()
            .filter_map(|m| match m {
                Move::Arc { ccw, .. } => Some(*ccw),
                _ => None,
            })
            .collect();
        assert!(!ccws.is_empty());
        assert!(ccws.iter().all(|&c| c == want_ccw), "{dir:?}");
        let text = &export(&pr, "c").unwrap()[0].text;
        let st = check_program(text, Units::Mm, 40.0);
        assert_eq!(st.arcs, ccws.len());
        assert_eq!(if want_ccw { st.g3 } else { st.g2 }, st.arcs, "{dir:?}");
        // tab lifts present (to Z-2 on the Z-3 pass)
        assert!(code(text)
            .iter()
            .any(|l| l.starts_with("G1") && l.ends_with("Z-2 F200")));
    }
}

// ---------------------------------------------------------------- F modality

#[test]
fn feed_modality_rules() {
    // F persists across ops of one file and across a drill cycle (F is modal in G-code too)
    let drill = Move::Drill {
        at: p(5.0, 0.0),
        z_bottom: -2.0,
        peck: None,
        r_plane: 2.0,
        feed: 600.0,
    };
    let t = LinuxCnc.program(&job(
        Units::Mm,
        vec![
            vec![Move::Rapid(p3(0.0, 0.0, 5.0)), lin(0.0, 0.0, -1.0, 600.0)],
            vec![
                Move::Rapid(p3(5.0, 0.0, 5.0)),
                drill,
                lin(5.0, 0.0, 1.0, 600.0),
                lin(5.0, 1.0, 1.0, 600.0004), // same formatted value → no F
                lin(5.0, 2.0, 1.0, 600.0006), // "600.001" → F
            ],
        ],
    ));
    assert_eq!(
        moves_of(&t),
        [
            "G0 X0 Y0 Z5",
            "G1 X0 Y0 Z-1 F600",
            "G0 X5 Y0 Z5",
            "G81 X5 Y0 Z-2 R2",
            "G80",
            "G1 X5 Y0 Z1",
            "G1 X5 Y1 Z1",
            "G1 X5 Y2 Z1 F600.001",
        ]
    );
}

#[test]
fn every_file_starts_its_own_feed() {
    // A, B, A with identical feeds: the first feed move of every file carries F
    let files = export(&line_ops(&[1, 2, 1]), "f").unwrap();
    assert_eq!(files.len(), 3);
    for f in &files {
        let first = code(&f.text)
            .into_iter()
            .find(|l| l.starts_with("G1 ") || l.starts_with("G81 ") || l.starts_with("G83 "))
            .unwrap();
        assert!(first.contains(" F"), "{}: {first}", f.suffix);
    }
}

// ---------------------------------------------------------------- segments

#[test]
fn segment_splitting() {
    let suffixes = |pr: &Project| -> Vec<String> {
        export(pr, "s")
            .unwrap()
            .into_iter()
            .map(|f| f.suffix)
            .collect()
    };
    // A, A, B, A → 3 files in user order
    assert_eq!(
        suffixes(&line_ops(&[1, 1, 2, 1])),
        ["_1_T1", "_2_T2", "_3_T1"]
    );
    // a disabled B between two A ops does not split; disabled first / last ops are skipped
    let mut pr = line_ops(&[2, 1, 2, 1, 2]);
    for k in [0, 2, 4] {
        pr.operations[k].enabled = false;
    }
    let files = export(&pr, "s").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].suffix, "");
    assert!(files[0].text.contains("(Op: op2)\n(Op: op4)\n"));
    assert!(!files[0].text.contains("op1") && !files[0].text.contains("op3"));
    // tool order not sorted: B, A → T2 first
    assert_eq!(suffixes(&line_ops(&[2, 1])), ["_1_T2", "_2_T1"]);
    // 9 segments → one digit, 10 → two digits, sorting by name = machining order
    let nine: Vec<u32> = (0..9).map(|k| 1 + k % 2).collect();
    assert_eq!(suffixes(&line_ops(&nine))[8], "_9_T1");
    let ten: Vec<u32> = (0..10).map(|k| 1 + k % 2).collect();
    let s = suffixes(&line_ops(&ten));
    assert_eq!(s[0], "_01_T1");
    assert_eq!(s[9], "_10_T2");
    let mut sorted = s.clone();
    sorted.sort();
    assert_eq!(sorted, s);
    // each file's header names its own tool and op list
    let files = export(&line_ops(&[1, 1, 2, 1]), "s").unwrap();
    assert!(files[1]
        .text
        .contains("\n(Tool: T2 Tool 2)\n(Op: op3)\n(Before"));
    assert!(files[2]
        .text
        .contains("\n(Tool: T1 Tool 1)\n(Op: op4)\n(Before"));
}

#[test]
fn first_failing_enabled_op_is_reported_and_nothing_exported() {
    let mut pr = Project::new();
    let sq = add(&mut pr, Geom::Contour(rect(0.0, 0.0, 20.0, 20.0)));
    let inside = OpKind::Contour {
        side: Side::Inside,
        direction: Direction::Climb,
    };
    let ok = tool(1, "ok", 3.0);
    let huge = tool(2, "huge", 50.0);
    op(&mut pr, "good", inside.clone(), &ok, vec![sq]);
    op(&mut pr, "bad disabled", inside.clone(), &huge, vec![sq]);
    op(&mut pr, "bad 1", inside.clone(), &huge, vec![sq]);
    op(&mut pr, "bad 2", inside, &huge, vec![sq]);
    pr.operations[1].enabled = false;
    // M8: every failing enabled op is reported (in user order), the disabled one is not
    let (bad1, bad2) = (pr.operations[2].id, pr.operations[3].id);
    match export(&pr, "x") {
        Err(PostError::Blocked(v)) => {
            let got: Vec<_> = v.iter().map(|i| (i.rule, i.op, i.item)).collect();
            assert_eq!(
                got,
                [
                    (RuleId::ToolTooLarge, Some(bad1), Some(sq)),
                    (RuleId::ToolTooLarge, Some(bad2), Some(sq))
                ]
            );
        }
        other => panic!("{other:?}"),
    }
}

/// Export is blocked by exactly one InvalidMachine issue (M7: ToolChangeBelowSafe).
fn machine_blocked(pr: &Project) -> bool {
    match export(pr, "x") {
        Err(PostError::Blocked(v)) => v.len() == 1 && v[0].rule == RuleId::InvalidMachine,
        _ => false,
    }
}

#[test]
fn tool_change_z_rules() {
    let mut pr = mixed();
    pr.machine.tool_change_z = 5.0 - 1e-12; // within tolerance
    assert!(export(&pr, "x").is_ok());
    pr.machine.tool_change_z = 4.999;
    assert!(machine_blocked(&pr));
    for v in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        pr.machine.tool_change_z = v;
        assert!(machine_blocked(&pr), "{v}");
    }
    // M8 order: NothingToExport comes before check, so an all-disabled project reports that
    for o in &mut pr.operations {
        o.enabled = false;
    }
    pr.machine.tool_change_z = 1.0;
    assert_eq!(export(&pr, "x"), Err(PostError::NothingToExport));
    // inch footer
    let mut pr = mixed();
    pr.units = Units::Inch;
    pr.machine.tool_change_z = 50.8;
    for f in export(&pr, "x").unwrap() {
        assert!(f.text.ends_with("\nG0 Z2\nM5\nM2\n"));
    }
}

#[test]
fn bad_toolpath_guard_non_finite_stock_origin() {
    let mut pr = mixed();
    pr.stock.origin = WorkOrigin::StockBottomLeft;
    pr.stock.min_x = f64::NAN;
    // M8: rule InvalidStock catches it before the BadToolpath guard
    match export(&pr, "x") {
        Err(PostError::Blocked(v)) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].rule, RuleId::InvalidStock);
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------- naming / comments

#[test]
fn stem_in_header_is_sanitized() {
    let pr = line_ops(&[1]);
    for (stem, want) in [
        ("My part (v2)", "(My part [v2] - file 1 of 1)"),
        ("Häuschen", "(H?uschen - file 1 of 1)"),
        ("", "(- - file 1 of 1)"),
        ("a;b", "(a;b - file 1 of 1)"),
    ] {
        let f = export(&pr, stem).unwrap();
        assert_eq!(f[0].suffix, "");
        assert_eq!(f[0].text.lines().next().unwrap(), want);
    }
}

#[test]
fn sanitize_comment_edges() {
    assert_eq!(sanitize_comment("a(b)c"), "a[b]c");
    assert_eq!(sanitize_comment("(())"), "[[]]");
    assert_eq!(sanitize_comment("x;y"), "x;y");
    assert_eq!(sanitize_comment("line1\nline2\r\n"), "line1 line2");
    assert_eq!(sanitize_comment("\u{7f}x\u{0}"), "x");
    assert_eq!(sanitize_comment("Ø"), "?");
    assert_eq!(sanitize_comment("日本"), "??"); // one '?' per char, not per byte
    assert_eq!(sanitize_comment("\u{1F600}"), "?");
    assert_eq!(sanitize_comment("   "), "-");
    let long = "ab(".repeat(70);
    let s = sanitize_comment(&long);
    assert_eq!(s.len(), 60);
    assert!(s.is_ascii() && !s.contains('('));
    // non-ASCII at the cut does not panic and counts as one char
    let s = sanitize_comment(&format!("{}ØØ", "a".repeat(59)));
    assert_eq!(s, format!("{}?", "a".repeat(59)));
    // names in every header line of a real file
    let mut pr = line_ops(&[1]);
    pr.operations[0].name = "Cut ) here\n(x)".into();
    pr.tools[0].name = "Bit\t(Ø3)".into();
    let t = &export(&pr, "n").unwrap()[0].text;
    assert!(t.contains("\n(Tool: T1 Bit [?3])\n"));
    assert!(t.contains("\n(Op: Cut ] here [x])\n"));
    assert!(t.contains("\n(Cut ] here [x])\n"));
    assert!(t.contains("\n(Before running: load T1 Bit [?3] and set Z0 on top of stock)\n"));
    check_program(t, Units::Mm, 40.0);
}

#[test]
fn empty_tool_name_header() {
    // spec: sanitize "" → "-"
    let mut pr = line_ops(&[1]);
    pr.tools[0].name = String::new();
    let t = &export(&pr, "n").unwrap()[0].text;
    assert!(t.contains("\n(Tool: T1 -)\n"), "{t}");
}

// ---------------------------------------------------------------- work origin

#[test]
fn work_origin_shifts_xy_of_every_move_but_not_z() {
    let base = mixed();
    let plain = export(&base, "o").unwrap();
    for (origin, (ox, oy)) in [
        (WorkOrigin::StockBottomLeft, (-7.25, 3.5)),
        (WorkOrigin::StockCenter, (-7.25 + 50.0, 3.5 + 20.0)),
    ] {
        let mut pr = base.clone();
        pr.stock.origin = origin;
        pr.stock.min_x = -7.25;
        pr.stock.min_y = 3.5;
        pr.stock.width = 100.0;
        pr.stock.height = 40.0;
        assert_eq!(work_origin(&pr), p(ox, oy));
        let shifted = export(&pr, "o").unwrap();
        assert_eq!(shifted.len(), plain.len());
        for (a, b) in plain.iter().zip(&shifted) {
            let (ca, cb) = (code(&a.text), code(&b.text));
            assert_eq!(ca.len(), cb.len());
            for (la, lb) in ca.iter().zip(&cb) {
                assert_eq!(la.split(' ').next(), lb.split(' ').next());
                for (k, off) in [('X', ox), ('Y', oy)] {
                    match (word(la, k), word(lb, k)) {
                        (Some(va), Some(vb)) => {
                            assert!((va - off - vb).abs() < 1.5e-3, "{la} / {lb}")
                        }
                        (None, None) => {}
                        _ => panic!("{la} / {lb}"),
                    }
                }
                for k in ['Z', 'I', 'J', 'R', 'Q', 'F', 'S'] {
                    assert_eq!(word(la, k), word(lb, k), "{la} / {lb}");
                }
            }
        }
        // the drill cycle is shifted too
        let drill = code(&shifted[1].text)
            .into_iter()
            .find(|l| l.starts_with("G83"))
            .unwrap();
        assert_eq!(
            drill,
            format!(
                "G83 X{} Y{} Z-3 R2 Q1 F200",
                fmt_num(10.0 - ox, 3),
                fmt_num(30.0 - oy, 3)
            )
        );
    }
}

#[test]
fn stock_center_in_inch() {
    let mut pr = mixed();
    pr.units = Units::Inch;
    pr.stock.origin = WorkOrigin::StockCenter;
    pr.stock.min_x = 0.0;
    pr.stock.min_y = 0.0;
    pr.stock.width = 50.8;
    pr.stock.height = 25.4;
    // hole (10,30) - (25.4,12.7) = (-15.4, 17.3) mm = (-0.6063, 0.6811) in
    let f = export(&pr, "o").unwrap();
    assert!(f[1].text.contains("\nG0 X-0.6063 Y0.6811 Z0.1969\nG83 X-0.6063 Y0.6811 Z-0.1181 R0.0787 Q0.0394 F7.874\nG80\n"), "{}", f[1].text);
    assert!(f[0].text.contains("XY0 = stock center)"));
}

// ---------------------------------------------------------------- project format v2 → v3

const GOLDEN_V2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/project/small_v2.hcam"
);

#[test]
fn v2_migrate_tool_change_z() {
    // hc-post has no serde_json dev-dependency: edit the JSON text
    let v2 = fs::read_to_string(GOLDEN_V2).unwrap();
    assert!(v2.contains("\"format_version\": 2,"));
    assert!(!v2.contains("tool_change_z"));
    let with_safe = |safe: &str| v2.replace("\"safe_z\": 5.0,", &format!("\"safe_z\": {safe},"));
    for (safe, want) in [
        ("5.0", 40.0),
        ("40.0", 40.0),
        ("50.0", 50.0),
        ("39.999", 40.0),
    ] {
        let (pr, _) = Project::from_json(&with_safe(safe)).unwrap();
        assert_eq!(pr.machine.tool_change_z, want, "safe {safe}");
        assert_eq!(pr.format_version, 4); // M10: was 3
    }
    // a v2 file that already has tool_change_z keeps it
    let (pr, _) = Project::from_json(&with_safe("5.0, \"tool_change_z\": 12.0")).unwrap();
    assert_eq!(pr.machine.tool_change_z, 12.0);
    // a migrated project never fails the tool change rule, even with a high safe Z
    let (pr, _) = Project::from_json(&with_safe("60.0")).unwrap();
    assert_eq!(pr.machine.tool_change_z, 60.0);
    assert!(!machine_blocked(&pr));
}

#[test]
fn v3_without_tool_change_z_is_a_json_error() {
    // spec: migrate only for found < 3, so a v3 file without the field is malformed
    // M10: the current version is 4; make it a v3 file
    let text = Project::new()
        .to_json()
        .unwrap()
        .replace("\"format_version\": 4,", "\"format_version\": 3,");
    assert!(text.contains("\"format_version\": 3,"));
    assert!(text.contains("\"tool_change_z\": 40.0,"));
    let r = Project::from_json(&text.replace("\"tool_change_z\": 40.0,", ""));
    assert!(
        matches!(r, Err(ProjectError::Json(_))),
        "{:?}",
        r.map(|(p, _)| p.machine)
    );
}

// ---------------------------------------------------------------- M10 test 7: picked work origin

#[test]
fn m10_07_picked_point_origin() {
    let base = mixed();
    let plain = export(&base, "o").unwrap();
    let mut pr = base.clone();
    pr.stock.origin = WorkOrigin::Point(p(10.0, 5.0));
    assert_eq!(work_origin(&pr), p(10.0, 5.0));
    let shifted = export(&pr, "o").unwrap();
    assert_eq!(shifted.len(), plain.len());
    for (a, b) in plain.iter().zip(&shifted) {
        assert!(b.text.contains("XY0 = drawing point X10 Y5)"), "{}", b.text);
        let (ca, cb) = (code(&a.text), code(&b.text));
        assert_eq!(ca.len(), cb.len());
        for (la, lb) in ca.iter().zip(&cb) {
            for (k, off) in [('X', 10.0), ('Y', 5.0)] {
                match (word(la, k), word(lb, k)) {
                    (Some(va), Some(vb)) => assert!((va - off - vb).abs() < 1e-3, "{la} / {lb}"),
                    (None, None) => {}
                    _ => panic!("{la} / {lb}"),
                }
            }
        }
    }
    pr.units = Units::Inch;
    let f = export(&pr, "o").unwrap();
    assert!(f[0].text.contains("XY0 = drawing point X0.3937 Y0.1969)"));
}
