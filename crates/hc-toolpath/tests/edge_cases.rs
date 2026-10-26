//! M5 edge cases (tester): global invariants, direction table, vanishing boundaries, pocket topologies,
//! depth-pass boundaries, drill variants, validation, extreme sizes. Machine safe 5, clearance 2.

use hc_geom::{Circle, Contour, Orientation, Point, Segment};
use hc_project::{Direction, Geom, Item, OpKind, Operation, Project, Side, Source, Tool, ToolKind};
use hc_toolpath::*;
use std::f64::consts::{PI, TAU};

const T: f64 = 1e-6;
const BRACKET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dxf/autocad2018_bracket_inch.dxf"
);

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn tool(id: u32, d: f64) -> Tool {
    Tool {
        id,
        name: format!("Flat {id}"),
        kind: ToolKind::FlatEndMill,
        diameter: d,
        flute_length: 20.0,
        flutes: 2,
        feed: 800.0,
        plunge_feed: 200.0,
        rpm: 18000,
        depth_per_pass: 2.0,
        stepover_pct: 50.0,
        comment: String::new(),
    }
}

fn poly(v: &[(f64, f64)], closed: bool) -> Contour {
    let v: Vec<(Point, f64)> = v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
    Contour::from_bulge_vertices(&v, closed)
}

fn rect(x0: f64, y0: f64, w: f64, h: f64) -> Contour {
    poly(
        &[(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)],
        true,
    )
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

fn add_c(pr: &mut Project, c: Contour) -> u64 {
    add(pr, Geom::Contour(c))
}

/// Project with T1 of diameter `d`.
fn project(d: f64) -> Project {
    let mut pr = Project::new();
    pr.tools.push(tool(1, d));
    pr
}

/// Op with T1, depth 5, dpp 5 (one level), feed 800, plunge 200; `kind` set verbatim.
fn add_op(pr: &mut Project, kind: OpKind, items: Vec<u64>) -> u64 {
    let t = pr.tools[0].clone();
    let id = pr.add_operation("op", kind.clone(), &t, items);
    let op = pr.operations.last_mut().unwrap();
    op.kind = kind;
    op.depth = 5.0;
    op.depth_per_pass = 5.0;
    id
}

fn op_mut(pr: &mut Project, id: u64) -> &mut Operation {
    pr.operations.iter_mut().find(|o| o.id == id).unwrap()
}

fn run(pr: &Project, id: u64) -> Result<Toolpath, ToolpathError> {
    generate_op(pr, pr.operations.iter().find(|o| o.id == id).unwrap())
}

fn ck(side: Side, direction: Direction) -> OpKind {
    OpKind::Contour { side, direction }
}

fn arc_sweep(from: Point, to: Point, center: Point, ccw: bool) -> f64 {
    let a0 = (from.y - center.y).atan2(from.x - center.x);
    let a1 = (to.y - center.y).atan2(to.x - center.x);
    if ccw {
        (a1 - a0).rem_euclid(TAU)
    } else {
        -(a0 - a1).rem_euclid(TAU)
    }
}

/// XY cut runs (z, path): every plunge starts a new run.
fn cut_runs(tp: &Toolpath) -> Vec<(f64, Contour)> {
    let mut runs: Vec<(f64, Vec<Segment>)> = Vec::new();
    let mut pos = Point3 {
        x: f64::NAN,
        y: f64::NAN,
        z: f64::NAN,
    };
    for m in &tp.moves {
        match m {
            Move::Rapid(to) => pos = *to,
            Move::Linear { to, .. } => {
                if to.z < pos.z - 1e-9 {
                    runs.push((to.z, Vec::new()));
                } else if !to.xy().approx_eq(pos.xy(), 1e-12) {
                    runs.last_mut().unwrap().1.push(Segment::Line {
                        start: pos.xy(),
                        end: to.xy(),
                    });
                }
                pos = *to;
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                let d = pos.xy() - *center;
                runs.last_mut().unwrap().1.push(Segment::Arc {
                    center: *center,
                    radius: d.length(),
                    start_angle: d.y.atan2(d.x),
                    sweep: arc_sweep(pos.xy(), to.xy(), *center, *ccw),
                });
                pos = *to;
            }
            Move::Drill { .. } => {}
        }
    }
    runs.into_iter()
        .map(|(z, segments)| {
            let closed = !segments.is_empty()
                && segments[0]
                    .start()
                    .approx_eq(segments.last().unwrap().end(), T);
            (z, Contour { segments, closed })
        })
        .collect()
}

fn radius_of(c: &Contour) -> f64 {
    c.bbox().width() / 2.0
}

/// Strict global invariants. `tol` scales the position checks for huge coordinates.
fn invariants(pr: &Project, tp: &Toolpath, tol: f64) {
    let m = &pr.machine;
    let op = pr.operations.iter().find(|o| o.id == tp.op_id).unwrap();
    assert_eq!(tp.tool_id, op.tool);
    assert_eq!(tp.rpm, op.rpm);
    assert!(!tp.moves.is_empty());
    let Move::Rapid(first) = tp.moves[0] else {
        panic!("first move must be a rapid: {:?}", tp.moves[0])
    };
    assert!((first.z - m.safe_z).abs() < 1e-9, "first move at safe_z");
    let mut pos = first;
    let mut plunged = false; // inside material since the last plunge
    for (i, mv) in tp.moves.iter().enumerate().skip(1) {
        let ctx = format!("move {i}: {mv:?} from {pos:?}");
        match mv {
            Move::Rapid(to) => {
                let xy = !to.xy().approx_eq(pos.xy(), tol);
                let dz = (to.z - pos.z).abs() > 1e-9;
                assert!(!(xy && dz), "diagonal rapid {ctx}");
                assert!(
                    to.z >= m.clearance_z - 1e-9,
                    "rapid ends below clearance {ctx}"
                );
                if xy {
                    assert!(
                        pos.z >= m.clearance_z - 1e-9,
                        "XY rapid below clearance {ctx}"
                    );
                }
                assert!(
                    to.z >= pos.z - 1e-9 || pos.z >= m.clearance_z - 1e-9,
                    "rapid downward from inside material {ctx}"
                );
                plunged = false;
                pos = *to;
            }
            Move::Linear { to, feed } => {
                assert!(to.z <= pos.z + 1e-9, "linear moving up {ctx}");
                if to.z < pos.z - 1e-9 {
                    assert!(
                        to.xy().approx_eq(pos.xy(), tol),
                        "plunge not vertical {ctx}"
                    );
                    assert_eq!(*feed, op.plunge_feed, "{ctx}");
                    assert!(to.z < 0.0, "plunge must end in material {ctx}");
                    plunged = true;
                } else {
                    assert!(
                        !to.xy().approx_eq(pos.xy(), 1e-12),
                        "zero-length linear {ctx}"
                    );
                    assert_eq!(*feed, op.feed, "{ctx}");
                    assert!(plunged && to.z < 0.0, "XY feed move not at cut depth {ctx}");
                }
                pos = *to;
            }
            Move::Arc {
                to,
                center,
                ccw,
                feed,
            } => {
                assert_eq!(*feed, op.feed, "{ctx}");
                assert!(plunged && to.z < 0.0, "arc not at cut depth {ctx}");
                assert!((to.z - pos.z).abs() < 1e-12, "arc changes z {ctx}");
                let (r0, r1) = (pos.xy().distance(*center), to.xy().distance(*center));
                assert!((r0 - r1).abs() < tol, "arc radius mismatch {r0} {r1} {ctx}");
                assert!(r0 > 1e-9, "zero-radius arc {ctx}");
                let sweep = arc_sweep(pos.xy(), to.xy(), *center, *ccw);
                assert!(sweep.abs() <= PI + 1e-9, "sweep {sweep} {ctx}");
                assert!(!to.xy().approx_eq(pos.xy(), 1e-9), "full/zero arc {ctx}");
                pos = *to;
            }
            Move::Drill {
                at,
                z_bottom,
                peck,
                r_plane,
                feed,
            } => {
                assert!(at.approx_eq(pos.xy(), tol), "{ctx}");
                assert!(
                    (pos.z - m.safe_z).abs() < 1e-9,
                    "drill starts at safe_z {ctx}"
                );
                assert_eq!(*z_bottom, -op.depth);
                assert_eq!(*r_plane, m.clearance_z);
                assert_eq!(*feed, op.plunge_feed);
                assert_eq!(
                    *peck,
                    match op.kind {
                        OpKind::Drill { peck } => peck,
                        _ => panic!("drill move in a non-drill op"),
                    }
                );
            }
        }
        assert!(pos.z >= -op.depth - 1e-9, "below final depth {ctx}");
    }
    assert!((pos.z - m.safe_z).abs() < 1e-9, "ends at safe_z");
}

fn bracket() -> Project {
    let imp = hc_dxf::import_dxf_file(std::path::Path::new(BRACKET), &Default::default()).unwrap();
    let mut pr = Project::new();
    pr.add_dxf(&imp, "bracket.dxf");
    pr.tools.push(tool(1, 6.35));
    pr
}

// ---------- global invariants on real data ----------

#[test]
fn bracket_all_op_kinds_invariants() {
    let mut pr = bracket();
    let contours: Vec<(u64, bool)> = pr
        .items
        .iter()
        .filter_map(|i| match &i.geom {
            Geom::Contour(c) => Some((i.id, c.closed)),
            _ => None,
        })
        .collect();
    let all: Vec<u64> = contours.iter().map(|c| c.0).collect();
    let closed: Vec<u64> = contours.iter().filter(|c| c.1).map(|c| c.0).collect();
    let circles: Vec<u64> = pr
        .items
        .iter()
        .filter(|i| matches!(&i.geom, Geom::Contour(c) if c.as_circle().is_some()))
        .map(|i| i.id)
        .collect();
    assert!(!circles.is_empty() && closed.len() > 1);
    let mut ops = vec![
        add_op(&mut pr, OpKind::Engrave, all.clone()),
        add_op(&mut pr, ck(Side::On, Direction::Climb), all.clone()),
        add_op(&mut pr, ck(Side::On, Direction::Conventional), all),
        add_op(&mut pr, ck(Side::Outside, Direction::Climb), closed.clone()),
        add_op(
            &mut pr,
            ck(Side::Outside, Direction::Conventional),
            closed.clone(),
        ),
        add_op(&mut pr, ck(Side::Inside, Direction::Climb), closed.clone()),
        add_op(
            &mut pr,
            ck(Side::Inside, Direction::Conventional),
            closed.clone(),
        ),
        add_op(&mut pr, OpKind::Pocket { stepover_pct: 40.0 }, closed),
        add_op(&mut pr, OpKind::Drill { peck: Some(1.0) }, circles.clone()),
        add_op(&mut pr, OpKind::Drill { peck: None }, circles),
    ];
    for &id in &ops {
        let o = op_mut(&mut pr, id);
        o.depth = 3.0;
        o.depth_per_pass = 1.4; // 3 levels, zig-zag on open paths
    }
    ops.dedup();
    let res = generate(&pr);
    assert_eq!(res.len(), ops.len());
    for (id, r) in &res {
        let tp = r.as_ref().unwrap_or_else(|e| panic!("op {id}: {e}"));
        invariants(&pr, tp, T);
    }
}

#[test]
fn every_xy_rapid_target_is_next_feature_start_and_retract_after_feature() {
    // closed square Outside, 3 levels: no retract between levels
    let mut pr = project(6.0);
    let sq = add_c(&mut pr, rect(0.0, 0.0, 20.0, 20.0));
    let op = add_op(&mut pr, ck(Side::Outside, Direction::Climb), vec![sq]);
    op_mut(&mut pr, op).depth_per_pass = 2.0;
    let tp = run(&pr, op).unwrap();
    invariants(&pr, &tp, T);
    let rapids = tp
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Rapid(_)))
        .count();
    assert_eq!(rapids, 3, "{:#?}", tp.moves);
    // 2 rapids + 3 × (plunge + 4 lines + 4 arcs) + final rapid
    assert_eq!(tp.moves.len(), 2 + 3 * 9 + 1);
    let runs = cut_runs(&tp);
    let zs: Vec<f64> = runs.iter().map(|r| r.0).collect();
    assert!(zs
        .iter()
        .zip([-5.0 / 3.0, -10.0 / 3.0, -5.0])
        .all(|(a, b)| (a - b).abs() < 1e-12));
    for (_, c) in &runs {
        assert!(c.closed);
        assert!((c.signed_area() + 668.274334).abs() < T); // CW, 26² − (4−π)·3²
    }
}

// ---------- direction ----------

#[test]
fn on_closed_direction_and_input_independence() {
    for stored_cw in [false, true] {
        let mut sq = rect(0.0, 0.0, 20.0, 20.0);
        if stored_cw {
            sq.reverse();
        }
        for (dir, want) in [
            (Direction::Climb, Orientation::Cw),
            (Direction::Conventional, Orientation::Ccw),
        ] {
            let mut pr = project(6.0);
            let id = add_c(&mut pr, sq.clone());
            let op = add_op(&mut pr, ck(Side::On, dir), vec![id]);
            let runs = cut_runs(&run(&pr, op).unwrap());
            assert_eq!(runs.len(), 1);
            assert_eq!(
                runs[0].1.orientation(),
                Some(want),
                "{dir:?} cw={stored_cw}"
            );
            // On = centre on the line
            assert!((runs[0].1.signed_area().abs() - 400.0).abs() < T);
        }
    }
}

#[test]
fn direction_matrix_on_circles() {
    // Same table as t03 but with arc-only input (both stored orientations).
    for stored_cw in [false, true] {
        let mut c = circle(0.0, 0.0, 20.0);
        if stored_cw {
            c.reverse();
        }
        for (side, dir, want, r) in [
            (Side::Outside, Direction::Climb, Orientation::Cw, 13.0),
            (
                Side::Outside,
                Direction::Conventional,
                Orientation::Ccw,
                13.0,
            ),
            (Side::Inside, Direction::Climb, Orientation::Ccw, 7.0),
            (Side::Inside, Direction::Conventional, Orientation::Cw, 7.0),
        ] {
            let mut pr = project(6.0);
            let id = add_c(&mut pr, c.clone());
            let op = add_op(&mut pr, ck(side, dir), vec![id]);
            let tp = run(&pr, op).unwrap();
            invariants(&pr, &tp, T);
            let runs = cut_runs(&tp);
            assert_eq!(runs.len(), 1);
            assert_eq!(
                runs[0].1.orientation(),
                Some(want),
                "{side:?} {dir:?} {stored_cw}"
            );
            assert!((radius_of(&runs[0].1) - r).abs() < T);
            assert_eq!(runs[0].1.segments.len(), 2, "full circle split in two arcs");
        }
    }
}

#[test]
fn engrave_keeps_stored_direction() {
    for stored_cw in [false, true] {
        let mut sq = rect(0.0, 0.0, 10.0, 10.0);
        if stored_cw {
            sq.reverse();
        }
        let want = sq.orientation();
        let mut pr = project(6.0);
        let id = add_c(&mut pr, sq);
        let op = add_op(&mut pr, OpKind::Engrave, vec![id]);
        let runs = cut_runs(&run(&pr, op).unwrap());
        assert_eq!(runs[0].1.orientation(), want);
    }
}

// ---------- vanishing boundary ----------

#[test]
fn inside_slot_at_vanishing_boundary() {
    // 6 mm wide slot: tool 5.9 fits, 6.0 (exact) and 6.1 do not.
    for (d, ok) in [(5.9, true), (6.0, false), (6.1, false)] {
        let mut pr = project(d);
        let id = add_c(&mut pr, rect(0.0, 0.0, 20.0, 6.0));
        let op = add_op(&mut pr, ck(Side::Inside, Direction::Climb), vec![id]);
        let r = run(&pr, op);
        if ok {
            let tp = r.unwrap_or_else(|e| panic!("d {d}: {e}"));
            invariants(&pr, &tp, T);
            let b = cut_runs(&tp)[0].1.bbox();
            assert!((b.height() - (6.0 - d)).abs() < T, "d {d}: {b:?}");
            assert!((b.width() - (20.0 - d)).abs() < T, "d {d}: {b:?}");
        } else {
            assert_eq!(r, Err(ToolpathError::OffsetVanished(id)), "d {d}");
        }
    }
    // hole diameter == tool diameter
    let mut pr = project(6.0);
    let id = add_c(&mut pr, circle(0.0, 0.0, 6.0));
    let op = add_op(&mut pr, ck(Side::Inside, Direction::Climb), vec![id]);
    assert_eq!(run(&pr, op), Err(ToolpathError::OffsetVanished(id)));
    let pk = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![id]);
    assert_eq!(run(&pr, pk), Err(ToolpathError::OffsetVanished(id)));
}

#[test]
fn inside_dumbbell_two_loops_one_feature() {
    // two 20×20 squares joined by a 2 mm neck: Ø6 inside → 2 loops, cut as one feature
    let c = poly(
        &[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 9.0),
            (30.0, 9.0),
            (30.0, 0.0),
            (50.0, 0.0),
            (50.0, 20.0),
            (30.0, 20.0),
            (30.0, 11.0),
            (20.0, 11.0),
            (20.0, 20.0),
            (0.0, 20.0),
        ],
        true,
    );
    assert!(offset_closed(&c, -3.0).len() == 2);
    let mut pr = project(6.0);
    let id = add_c(&mut pr, c);
    let op = add_op(&mut pr, ck(Side::Inside, Direction::Climb), vec![id]);
    let tp = run(&pr, op).unwrap();
    invariants(&pr, &tp, T);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 2);
    for (_, r) in &runs {
        assert_eq!(r.orientation(), Some(Orientation::Ccw));
        // the neck corners (20,9),(20,11) let each loop bulge slightly past x = 17 / x = 33
        assert!(
            r.signed_area() > 196.0 && r.signed_area() < 197.0,
            "{}",
            r.signed_area()
        );
    }
    // one feature: only one retract to safe_z after the first move
    let safe = tp
        .moves
        .iter()
        .skip(1)
        .filter(|m| matches!(m, Move::Rapid(p) if p.z == pr.machine.safe_z))
        .count();
    assert_eq!(safe, 1, "{:#?}", tp.moves);
}

// ---------- pockets ----------

fn pocket_radii(pr: &Project, op: u64) -> Vec<(f64, Option<Orientation>)> {
    let tp = run(pr, op).unwrap();
    invariants(pr, &tp, T);
    cut_runs(&tp)
        .into_iter()
        .map(|(_, c)| (radius_of(&c), c.orientation()))
        .collect()
}

#[test]
fn pocket_stepover_100_and_1_percent() {
    let mut pr = project(6.0);
    let c = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let full = add_op(
        &mut pr,
        OpKind::Pocket {
            stepover_pct: 100.0,
        },
        vec![c],
    );
    let r: Vec<f64> = pocket_radii(&pr, full).iter().map(|x| x.0).collect();
    assert_eq!(r.len(), 5, "{r:?}");
    for (got, want) in r.iter().zip([3.0, 9.0, 15.0, 21.0, 27.0]) {
        assert!((got - want).abs() < T, "{r:?}");
    }
    let fine = add_op(&mut pr, OpKind::Pocket { stepover_pct: 1.0 }, vec![c]);
    let r = pocket_radii(&pr, fine);
    // 27, 26.94, …: radius 27 − 0.06k > 0 → k = 0..449
    assert!((449..=450).contains(&r.len()), "{}", r.len());
    assert!((r.last().unwrap().0 - 27.0).abs() < T);
    assert!(r.windows(2).all(|w| w[1].0 > w[0].0));
}

#[test]
fn pocket_stepover_cap_is_error_not_hang() {
    let mut pr = project(0.1);
    let c = add_c(&mut pr, rect(0.0, 0.0, 400.0, 400.0));
    let op = add_op(&mut pr, OpKind::Pocket { stepover_pct: 1.0 }, vec![c]);
    assert_eq!(
        run(&pr, op),
        Err(ToolpathError::InvalidParam("stepover_pct"))
    );
}

/// Island of Ø20 touching the Ø60 boundary internally; `ix` = ±20 picks the touch point.
fn touching_island(ix: f64) {
    let mut pr = project(6.0);
    let b = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let i = add_c(&mut pr, circle(ix, 0.0, 20.0));
    let op = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![b, i]);
    let tp = run(&pr, op).unwrap();
    invariants(&pr, &tp, T);
    let runs = cut_runs(&tp);
    assert!(!runs.is_empty());
    for (_, c) in &runs {
        for s in &c.segments {
            let q = s.start();
            assert!(
                q.distance(p(ix, 0.0)) >= 13.0 - T,
                "ring enters island at {q:?}"
            );
            assert!(
                q.distance(p(0.0, 0.0)) <= 27.0 + T,
                "ring outside boundary at {q:?}"
            );
        }
    }
}

#[test]
fn pocket_island_touching_boundary_away_from_its_start() {
    // island start point (-10,0) is strictly inside the boundary; touch point (-30,0)
    touching_island(-20.0);
}

#[test]
fn pocket_island_touching_boundary_at_its_start() {
    // island start point (30,0) is exactly the touch point
    touching_island(20.0);
}

#[test]
fn pocket_two_separate_boundaries() {
    let mut pr = project(6.0);
    let far = add_c(&mut pr, circle(100.0, 0.0, 30.0));
    let near = add_c(&mut pr, circle(0.0, 0.0, 30.0));
    let op = add_op(
        &mut pr,
        OpKind::Pocket { stepover_pct: 50.0 },
        vec![far, near],
    );
    let tp = run(&pr, op).unwrap();
    invariants(&pr, &tp, T);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 8);
    let cx: Vec<f64> = runs
        .iter()
        .map(|(_, c)| c.bbox().min.x + radius_of(c))
        .collect();
    // nearest neighbour from (0,0): the circle at the origin first
    assert!(cx[..4].iter().all(|x| x.abs() < T), "{cx:?}");
    assert!(cx[4..].iter().all(|x| (x - 100.0).abs() < T), "{cx:?}");
    for k in [0, 4] {
        let r: Vec<f64> = runs[k..k + 4].iter().map(|(_, c)| radius_of(c)).collect();
        for (got, want) in r.iter().zip([3.0, 6.0, 9.0, 12.0]) {
            assert!((got - want).abs() < T, "{r:?}");
        }
    }
}

#[test]
fn pocket_nested_boundary_island_boundary() {
    let mut pr = project(6.0);
    let outer = add_c(&mut pr, circle(0.0, 0.0, 100.0));
    let island = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let inner = add_c(&mut pr, circle(0.0, 0.0, 20.0));
    let op = add_op(
        &mut pr,
        OpKind::Pocket { stepover_pct: 50.0 },
        vec![outer, island, inner],
    );
    let runs = pocket_radii(&pr, op);
    assert_eq!(runs.len(), 9, "{runs:?}");
    // depth-2 boundary first: r 1, 4, 7 (CCW, innermost level first)
    for ((r, o), want) in runs[..3].iter().zip([1.0, 4.0, 7.0]) {
        assert!((r - want).abs() < T, "{runs:?}");
        assert_eq!(*o, Some(Orientation::Ccw));
    }
    // then the annulus 30..50: levels (41,39), (44,36), (47,33), innermost level first
    let mut ccw: Vec<f64> = Vec::new();
    let mut cw: Vec<f64> = Vec::new();
    for (r, o) in &runs[3..] {
        match o {
            Some(Orientation::Ccw) => ccw.push(*r),
            Some(Orientation::Cw) => cw.push(*r),
            None => panic!("degenerate ring"),
        }
    }
    let close = |a: &[f64], b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < T)
    };
    assert!(close(&ccw, &[41.0, 44.0, 47.0]), "{ccw:?}");
    assert!(close(&cw, &[39.0, 36.0, 33.0]), "{cw:?}");
}

// ---------- depth passes ----------

#[test]
fn depth_levels_boundaries() {
    let eq = |a: Vec<f64>, b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    };
    assert!(eq(depth_levels(2.0, 2.0), &[-2.0]));
    assert!(eq(depth_levels(0.5, 2.0), &[-0.5]));
    // float noise just above a multiple must not add a pass
    assert!(eq(depth_levels(0.3 * 3.0, 0.3), &[-0.3, -0.6, -0.9]));
    assert!(eq(
        depth_levels(6.0 + 1e-10, 2.0),
        &[-2.0, -4.0, -6.0 - 1e-10]
    ));
    assert_eq!(depth_levels(6.0 + 1e-6, 2.0).len(), 4);
    // inch: 1/8" deep, 1/16" per pass
    assert!(eq(depth_levels(3.175, 1.5875), &[-1.5875, -3.175]));
    for (d, dpp) in [(5.0, 2.0), (7.3, 0.4), (0.1, 0.1), (25.4, 3.0)] {
        let l = depth_levels(d, dpp);
        let mut prev = 0.0;
        for z in &l {
            assert!(prev - z <= dpp + 1e-9 && *z < prev);
            prev = *z;
        }
        assert!((prev + d).abs() < 1e-12);
    }
}

#[test]
fn engrave_open_polyline_zigzag_three_passes() {
    let mut pr = project(3.0);
    let id = add_c(
        &mut pr,
        poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false),
    );
    let op = add_op(&mut pr, OpKind::Engrave, vec![id]);
    let o = op_mut(&mut pr, op);
    o.depth = 6.0;
    o.depth_per_pass = 2.0;
    let tp = run(&pr, op).unwrap();
    invariants(&pr, &tp, T);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 3);
    let a = p(0.0, 0.0);
    let b = p(10.0, 10.0);
    for ((z, c), (wz, s, e)) in runs.iter().zip([(-2.0, a, b), (-4.0, b, a), (-6.0, a, b)]) {
        assert!((z - wz).abs() < 1e-12);
        assert!(
            c.start().approx_eq(s, T) && c.end().approx_eq(e, T),
            "{c:?}"
        );
        assert_eq!(c.segments.len(), 2);
    }
    // no air moves: exactly the initial 2 rapids and the final retract
    assert_eq!(
        tp.moves
            .iter()
            .filter(|m| matches!(m, Move::Rapid(_)))
            .count(),
        3
    );
    let Move::Rapid(last) = tp.moves.last().unwrap() else {
        panic!()
    };
    assert!(last.xy().approx_eq(b, T) && last.z == 5.0);
}

#[test]
fn engrave_depth_equal_and_below_dpp_single_pass() {
    for (depth, dpp) in [(2.0, 2.0), (0.5, 2.0)] {
        let mut pr = project(3.0);
        let id = add_c(&mut pr, poly(&[(0.0, 0.0), (10.0, 0.0)], false));
        let op = add_op(&mut pr, OpKind::Engrave, vec![id]);
        let o = op_mut(&mut pr, op);
        o.depth = depth;
        o.depth_per_pass = dpp;
        let tp = run(&pr, op).unwrap();
        invariants(&pr, &tp, T);
        let runs = cut_runs(&tp);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, -depth);
        assert_eq!(tp.moves.len(), 5);
    }
}

// ---------- drill ----------

#[test]
fn drill_variants() {
    let mut pr = project(5.0);
    let c = add_c(&mut pr, circle(10.0, 10.0, 5.0));
    let pt = add(&mut pr, Geom::Point(p(-3.0, 4.0)));
    for peck in [None, Some(20.0), Some(0.5)] {
        let op = add_op(&mut pr, OpKind::Drill { peck }, vec![c, pt]);
        let o = op_mut(&mut pr, op);
        // ignored for drill
        o.depth_per_pass = 0.0;
        o.feed = f64::NAN;
        let tp = run(&pr, op).unwrap();
        invariants(&pr, &tp, T);
        assert_eq!(tp.moves.len(), 4);
        // nearest from (0,0): the point (dist 5) before the circle centre (dist 14.1)
        assert_eq!(
            tp.moves[0],
            Move::Rapid(Point3 {
                x: -3.0,
                y: 4.0,
                z: 5.0
            })
        );
        assert!(
            matches!(tp.moves[3], Move::Drill { at, peck: pk, .. } if at == p(10.0, 10.0) && pk == peck)
        );
    }
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let op = add_op(&mut pr, OpKind::Drill { peck: Some(bad) }, vec![c]);
        assert_eq!(
            run(&pr, op),
            Err(ToolpathError::InvalidParam("peck")),
            "{bad}"
        );
    }
    // a half-circle arc contour is not a drill target; neither is an open contour
    let half = add_c(
        &mut pr,
        Contour::from_bulge_vertices(&[(p(1.0, 0.0), 1.0), (p(-1.0, 0.0), 0.0)], true),
    );
    let op = add_op(&mut pr, OpKind::Drill { peck: None }, vec![c, half]);
    assert_eq!(run(&pr, op), Err(ToolpathError::UnsupportedItem(half)));
}

// ---------- validation / errors ----------

#[test]
fn validation_never_panics() {
    let mut base = project(6.0);
    let sq = add_c(&mut base, rect(0.0, 0.0, 20.0, 20.0));
    let kinds = [
        OpKind::Engrave,
        ck(Side::Outside, Direction::Climb),
        OpKind::Pocket { stepover_pct: 50.0 },
    ];
    type F = fn(&mut Project, u64);
    let cases: [(F, &str); 14] = [
        (|p, o| op_mut(p, o).depth = -1.0, "depth"),
        (|p, o| op_mut(p, o).depth = f64::NAN, "depth"),
        (|p, o| op_mut(p, o).depth_per_pass = -2.0, "depth_per_pass"),
        (
            |p, o| op_mut(p, o).depth_per_pass = f64::INFINITY,
            "depth_per_pass",
        ),
        (
            |p, o| op_mut(p, o).depth_per_pass = 5.0 / 10_001.0,
            "depth_per_pass",
        ),
        (|p, o| op_mut(p, o).feed = -800.0, "feed"),
        (|p, o| op_mut(p, o).feed = f64::NAN, "feed"),
        (|p, o| op_mut(p, o).plunge_feed = 0.0, "plunge_feed"),
        (
            |p, o| op_mut(p, o).plunge_feed = f64::NEG_INFINITY,
            "plunge_feed",
        ),
        (|p, _| p.machine.clearance_z = -1.0, "clearance_z"),
        (|p, _| p.machine.clearance_z = f64::NAN, "clearance_z"),
        (|p, _| p.machine.safe_z = f64::NAN, "safe_z"),
        (|p, _| p.machine.safe_z = 1.999, "safe_z"),
        // validation comes before the tool lookup
        (
            |p, o| {
                op_mut(p, o).tool = 99;
                op_mut(p, o).depth = 0.0
            },
            "depth",
        ),
    ];
    for kind in &kinds {
        for (f, name) in &cases {
            let mut pr = base.clone();
            let op = add_op(&mut pr, kind.clone(), vec![sq]);
            f(&mut pr, op);
            assert_eq!(
                run(&pr, op),
                Err(ToolpathError::InvalidParam(name)),
                "{kind:?} {name}"
            );
        }
    }
    // stepover out of range
    for so in [0.0, -10.0, 100.0001, f64::NAN] {
        let mut pr = base.clone();
        let op = add_op(&mut pr, OpKind::Pocket { stepover_pct: so }, vec![sq]);
        assert_eq!(
            run(&pr, op),
            Err(ToolpathError::InvalidParam("stepover_pct")),
            "{so}"
        );
    }
    // tool diameter: Contour/Pocket reject, Engrave/Drill don't care
    for d in [0.0, -6.0, f64::NAN] {
        let mut pr = base.clone();
        pr.tools[0].diameter = d;
        let c = add_op(&mut pr, ck(Side::Inside, Direction::Climb), vec![sq]);
        let pk = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![sq]);
        let e = add_op(&mut pr, OpKind::Engrave, vec![sq]);
        assert!(
            matches!(run(&pr, c), Err(ToolpathError::InvalidParam(_))),
            "{d}"
        );
        assert!(
            matches!(run(&pr, pk), Err(ToolpathError::InvalidParam(_))),
            "{d}"
        );
        assert!(run(&pr, e).is_ok(), "{d}");
    }
}

#[test]
fn op_level_errors() {
    let mut pr = project(6.0);
    let sq = add_c(&mut pr, rect(0.0, 0.0, 20.0, 20.0));
    let open = add_c(&mut pr, poly(&[(0.0, 0.0), (5.0, 5.0)], false));
    let pt = add(&mut pr, Geom::Point(p(1.0, 1.0)));
    let empty = add_c(
        &mut pr,
        Contour {
            segments: vec![],
            closed: true,
        },
    );

    let none = add_op(&mut pr, ck(Side::On, Direction::Climb), vec![]);
    assert_eq!(run(&pr, none), Err(ToolpathError::NoItems));
    let unknown = add_op(&mut pr, OpKind::Drill { peck: None }, vec![sq]);
    op_mut(&mut pr, unknown).tool = 42;
    assert_eq!(run(&pr, unknown), Err(ToolpathError::MissingTool(42)));
    for side in [Side::Inside, Side::Outside] {
        let op = add_op(&mut pr, ck(side, Direction::Climb), vec![sq, open]);
        assert_eq!(run(&pr, op), Err(ToolpathError::OpenContour(open)));
    }
    let on_open = add_op(&mut pr, ck(Side::On, Direction::Climb), vec![open]);
    assert!(run(&pr, on_open).is_ok());
    for kind in [
        OpKind::Engrave,
        ck(Side::On, Direction::Climb),
        ck(Side::Outside, Direction::Climb),
        OpKind::Pocket { stepover_pct: 50.0 },
    ] {
        let op = add_op(&mut pr, kind.clone(), vec![sq, pt]);
        assert_eq!(
            run(&pr, op),
            Err(ToolpathError::UnsupportedItem(pt)),
            "{kind:?}"
        );
        // empty contour item: error, not panic
        let op = add_op(&mut pr, kind.clone(), vec![empty]);
        assert!(run(&pr, op).is_err(), "{kind:?}");
    }
    let op = add_op(&mut pr, OpKind::Drill { peck: None }, vec![empty]);
    assert_eq!(run(&pr, op), Err(ToolpathError::UnsupportedItem(empty)));

    // disabled ops are skipped, order of the rest kept
    let mut q = project(6.0);
    let sq = add_c(&mut q, rect(0.0, 0.0, 20.0, 20.0));
    let a = add_op(&mut q, OpKind::Engrave, vec![sq]);
    let b = add_op(&mut q, OpKind::Engrave, vec![sq]);
    let c = add_op(&mut q, ck(Side::On, Direction::Climb), vec![]);
    op_mut(&mut q, b).enabled = false;
    let ids: Vec<u64> = generate(&q).iter().map(|r| r.0).collect();
    assert_eq!(ids, [a, c]);
    assert!(generate(&Project::new()).is_empty());
    // Display strings exist for every variant
    for e in [
        ToolpathError::MissingTool(1),
        ToolpathError::MissingItem(2),
        ToolpathError::NoItems,
        ToolpathError::InvalidParam("feed"),
        ToolpathError::UnsupportedItem(3),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

// ---------- extreme sizes ----------

#[test]
fn huge_coordinates() {
    let mut pr = project(6.0);
    let sq = add_c(&mut pr, rect(1e5, 1e5, 20.0, 20.0));
    let c = add_c(&mut pr, circle(1e5 + 50.0, 1e5, 30.0));
    let out = add_op(&mut pr, ck(Side::Outside, Direction::Climb), vec![sq]);
    let pk = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![c]);
    let tp = run(&pr, out).unwrap();
    invariants(&pr, &tp, 1e-6);
    let loop_ = &cut_runs(&tp)[0].1;
    let b = loop_.bbox();
    assert!(b.min.approx_eq(p(1e5 - 3.0, 1e5 - 3.0), 1e-6), "{b:?}");
    assert!(b.max.approx_eq(p(1e5 + 23.0, 1e5 + 23.0), 1e-6), "{b:?}");
    assert!((loop_.signed_area() + 668.274334).abs() < 1e-4);
    let tp = run(&pr, pk).unwrap();
    invariants(&pr, &tp, 1e-6);
    assert_eq!(cut_runs(&tp).len(), 4);
}

#[test]
fn tiny_circle_engrave_and_pocket() {
    let mut pr = project(0.05);
    let c = add_c(&mut pr, circle(5.0, 5.0, 0.1));
    let e = add_op(&mut pr, OpKind::Engrave, vec![c]);
    let o = op_mut(&mut pr, e);
    o.depth = 0.1;
    o.depth_per_pass = 0.1;
    let tp = run(&pr, e).unwrap();
    invariants(&pr, &tp, 1e-9);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, -0.1);
    assert_eq!(runs[0].1.segments.len(), 2);
    assert!((radius_of(&runs[0].1) - 0.05).abs() < 1e-9);

    let pk = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![c]);
    let r = pocket_radii(&pr, pk);
    assert!(!r.is_empty());
    assert!((r.last().unwrap().0 - 0.025).abs() < 1e-9, "{r:?}");

    let inside = add_op(&mut pr, ck(Side::Inside, Direction::Climb), vec![c]);
    pr.tools[0].diameter = 0.2;
    assert_eq!(run(&pr, inside), Err(ToolpathError::OffsetVanished(c)));
}
