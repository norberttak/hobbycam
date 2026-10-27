//! M5 acceptance tests (specs/M5.md), one test per item (16 and 17 live in the app).
//! Machine safe 5, clearance 2; tool T1 flat Ø6 unless noted.

use hc_geom::{Circle, Contour, Orientation, Point, Segment};
use hc_project::{Direction, Geom, Item, OpKind, Project, Side, Source, Tool, ToolKind};
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

fn line(a: Point, b: Point) -> Contour {
    Contour::from_bulge_vertices(&[(a, 0.0), (b, 0.0)], false)
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

/// New op with T1 Ø6 (added to the project if needed), depth 5, dpp 2, feed 800, plunge 200.
fn add_op(pr: &mut Project, kind: OpKind, items: Vec<u64>) -> u64 {
    let t = pr.tools.first().cloned().unwrap_or_else(|| tool(1, 6.0));
    let id = pr.add_operation("op", kind, &t, items);
    let op = pr.operations.last_mut().unwrap();
    op.depth = 5.0;
    id
}

fn op_mut(pr: &mut Project, id: u64) -> &mut hc_project::Operation {
    pr.operations.iter_mut().find(|o| o.id == id).unwrap()
}

fn run(pr: &Project, id: u64) -> Result<Toolpath, ToolpathError> {
    generate_op(pr, pr.operations.iter().find(|o| o.id == id).unwrap())
}

fn contour_kind(side: Side, direction: Direction) -> OpKind {
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

/// XY cut runs: every plunge starts a new run; feed moves with XY motion become segments.
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
                } else if !to.xy().approx_eq(pos.xy(), 1e-9) {
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
                let from = pos.xy();
                let d = from - *center;
                runs.last_mut().unwrap().1.push(Segment::Arc {
                    center: *center,
                    radius: d.length(),
                    start_angle: d.y.atan2(d.x),
                    sweep: arc_sweep(from, to.xy(), *center, *ccw),
                });
                pos = *to;
            }
            Move::Drill { .. } => {}
        }
    }
    runs.into_iter()
        .map(|(z, segments)| {
            let closed = segments
                .first()
                .unwrap()
                .start()
                .approx_eq(segments.last().unwrap().end(), T);
            (z, Contour { segments, closed })
        })
        .collect()
}

fn radius_of(c: &Contour) -> f64 {
    c.bbox().width() / 2.0
}

#[test]
fn t01_offset_square() {
    let sq = rect(0.0, 0.0, 20.0, 20.0);
    let r = offset_closed(&sq, 3.0);
    assert_eq!(r.len(), 1);
    let c = &r[0];
    assert!(c.closed);
    let lines = c
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::Line { .. }))
        .count();
    let arcs: Vec<f64> = c
        .segments
        .iter()
        .filter_map(|s| match *s {
            Segment::Arc { radius, .. } => Some(radius),
            _ => None,
        })
        .collect();
    assert_eq!((lines, arcs.len()), (4, 4));
    assert!(arcs.iter().all(|r| (r - 3.0).abs() < T));
    assert!((c.signed_area().abs() - 668.274334).abs() < T);
    assert!((c.signed_area().abs() - (400.0 + 240.0 + 9.0 * PI)).abs() < 1e-9);
    let b = c.bbox();
    assert!(b.min.approx_eq(p(-3.0, -3.0), T) && b.max.approx_eq(p(23.0, 23.0), T));
    assert_eq!(c.orientation(), Some(Orientation::Ccw));

    let mut cw = sq.clone();
    cw.reverse();
    let r2 = offset_closed(&cw, 3.0);
    assert_eq!(r2.len(), 1);
    assert!((r2[0].signed_area() - c.signed_area()).abs() < T);

    let r3 = offset_closed(&sq, -3.0);
    assert_eq!(r3.len(), 1);
    assert!((r3[0].signed_area() - 196.0).abs() < T);
    assert_eq!(r3[0].segments.len(), 4);
    assert!(r3[0]
        .segments
        .iter()
        .all(|s| matches!(s, Segment::Line { .. })));
    let b = r3[0].bbox();
    assert!((b.width() - 14.0).abs() < T && (b.height() - 14.0).abs() < T);
}

#[test]
fn t02_offset_circle() {
    let r = offset_closed(&circle(0.0, 0.0, 20.0), -3.0);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].segments.len(), 2);
    for s in &r[0].segments {
        let Segment::Arc { radius, center, .. } = *s else {
            panic!("expected arcs")
        };
        assert!((radius - 7.0).abs() < T && center.approx_eq(p(0.0, 0.0), T));
    }
    assert!((r[0].signed_area() - 49.0 * PI).abs() < T);
    assert!(offset_closed(&circle(0.0, 0.0, 5.0), -3.0).is_empty());
}

#[test]
fn t03_direction() {
    for stored_cw in [false, true] {
        let mut sq = rect(0.0, 0.0, 20.0, 20.0);
        if stored_cw {
            sq.reverse();
        }
        let cases = [
            (Side::Outside, Direction::Climb, Orientation::Cw),
            (Side::Outside, Direction::Conventional, Orientation::Ccw),
            (Side::Inside, Direction::Climb, Orientation::Ccw),
            (Side::Inside, Direction::Conventional, Orientation::Cw),
        ];
        for (side, dir, want) in cases {
            let mut pr = Project::new();
            let id = add_c(&mut pr, sq.clone());
            let op = add_op(&mut pr, contour_kind(side, dir), vec![id]);
            let runs = cut_runs(&run(&pr, op).unwrap());
            assert_eq!(
                runs[0].1.orientation(),
                Some(want),
                "{side:?} {dir:?} cw={stored_cw}"
            );
        }
    }
    for (dir, sign) in [(Direction::Climb, 1.0), (Direction::Conventional, -1.0)] {
        let mut pr = Project::new();
        let id = add_c(&mut pr, line(p(0.0, 0.0), p(10.0, 0.0)));
        let op = add_op(&mut pr, contour_kind(Side::On, dir), vec![id]);
        let runs = cut_runs(&run(&pr, op).unwrap());
        let s = &runs[0].1.segments[0];
        assert!((s.end().x - s.start().x) * sign > 0.0, "{dir:?}");
    }
}

#[test]
fn t04_offset_vanished_is_per_op() {
    let mut pr = Project::new();
    let small = add_c(&mut pr, circle(0.0, 0.0, 5.0));
    let sq = add_c(&mut pr, rect(10.0, 10.0, 20.0, 20.0));
    let a = add_op(
        &mut pr,
        contour_kind(Side::Inside, Direction::Climb),
        vec![small],
    );
    let b = add_op(
        &mut pr,
        contour_kind(Side::Outside, Direction::Climb),
        vec![sq],
    );
    let all = generate(&pr);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], (a, Err(ToolpathError::OffsetVanished(small))));
    assert_eq!(all[1].0, b);
    assert!(all[1].1.is_ok());
}

#[test]
fn t05_pocket_ring_counts() {
    let levels = pocket_rings(&circle(0.0, 0.0, 60.0), &[], 3.0, 3.0);
    assert_eq!(levels.len(), 9);
    for (i, l) in levels.iter().enumerate() {
        assert_eq!(l.len(), 1);
        assert!(
            (radius_of(&l[0]) - (27.0 - 3.0 * i as f64)).abs() < T,
            "level {i}"
        );
    }
    let levels = pocket_rings(&rect(-20.0, -20.0, 40.0, 40.0), &[], 3.0, 3.0);
    let half: Vec<f64> = levels.iter().map(|l| radius_of(&l[0])).collect();
    assert_eq!(levels.len(), 6, "{half:?}");
    for (h, want) in half.iter().zip([17.0, 14.0, 11.0, 8.0, 5.0, 2.0]) {
        assert!((h - want).abs() < T, "{half:?}");
    }
}

#[test]
fn t06_pocket_with_island() {
    let levels = pocket_rings(&circle(0.0, 0.0, 60.0), &[circle(0.0, 0.0, 20.0)], 3.0, 3.0);
    assert_eq!(levels.len(), 3);
    for (i, l) in levels.iter().enumerate() {
        assert_eq!(l.len(), 2);
        let ccw: Vec<_> = l
            .iter()
            .filter(|c| c.orientation() == Some(Orientation::Ccw))
            .collect();
        let cw: Vec<_> = l
            .iter()
            .filter(|c| c.orientation() == Some(Orientation::Cw))
            .collect();
        assert_eq!((ccw.len(), cw.len()), (1, 1));
        assert!((radius_of(ccw[0]) - (27.0 - 3.0 * i as f64)).abs() < T);
        assert!((radius_of(cw[0]) - (13.0 + 3.0 * i as f64)).abs() < T);
    }

    // via an op: island selected or not
    let mut pr = Project::new();
    let outer = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let inner = add_c(&mut pr, circle(0.0, 0.0, 20.0));
    let tiny = add_c(&mut pr, circle(100.0, 0.0, 5.0));
    let kind = OpKind::Pocket { stepover_pct: 50.0 };
    let both = add_op(&mut pr, kind.clone(), vec![outer, inner]);
    let only = add_op(&mut pr, kind.clone(), vec![outer]);
    let small = add_op(&mut pr, kind, vec![tiny]);
    for id in [both, only] {
        op_mut(&mut pr, id).depth_per_pass = 5.0;
    }
    assert_eq!(cut_runs(&run(&pr, both).unwrap()).len(), 6);
    assert_eq!(cut_runs(&run(&pr, only).unwrap()).len(), 9);
    assert_eq!(run(&pr, small), Err(ToolpathError::OffsetVanished(tiny)));
}

#[test]
fn t07_pocket_cut_order() {
    let mut pr = Project::new();
    let c = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let op = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![c]);
    op_mut(&mut pr, op).depth_per_pass = 5.0;
    let radii: Vec<f64> = cut_runs(&run(&pr, op).unwrap())
        .iter()
        .map(|(_, c)| radius_of(c))
        .collect();
    assert_eq!(radii.len(), 9);
    for (i, r) in radii.iter().enumerate() {
        assert!((r - 3.0 * (i + 1) as f64).abs() < T, "{radii:?}");
    }
}

#[test]
fn t08_depth_levels() {
    let close = |a: Vec<f64>, b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < T)
    };
    assert!(close(depth_levels(5.0, 2.0), &[-1.666667, -3.333333, -5.0]));
    assert!(close(depth_levels(6.0, 2.0), &[-2.0, -4.0, -6.0]));
    assert!(close(depth_levels(1.0, 2.0), &[-1.0]));

    let mut pr = Project::new();
    let id = add_c(&mut pr, line(p(0.0, 0.0), p(10.0, 0.0)));
    let op = add_op(&mut pr, OpKind::Engrave, vec![id]);
    for (f, name) in [
        (
            &(|o: &mut hc_project::Operation| o.depth = 0.0) as &dyn Fn(&mut hc_project::Operation),
            "depth",
        ),
        (&|o| o.depth_per_pass = 0.0, "depth_per_pass"),
        (&|o| o.depth = f64::NAN, "depth"),
        (&|o| o.depth_per_pass = f64::NAN, "depth_per_pass"),
    ] {
        let mut q = pr.clone();
        f(op_mut(&mut q, op));
        assert_eq!(run(&q, op), Err(ToolpathError::InvalidParam(name)));
    }
}

/// Moves equal within T.
fn same(a: &Move, b: &Move) -> bool {
    let pt = |a: &Point3, b: &Point3| {
        (a.x - b.x).abs() < T && (a.y - b.y).abs() < T && (a.z - b.z).abs() < T
    };
    match (a, b) {
        (Move::Rapid(x), Move::Rapid(y)) => pt(x, y),
        (Move::Linear { to: x, feed: f }, Move::Linear { to: y, feed: g }) => {
            pt(x, y) && (f - g).abs() < T
        }
        _ => a == b,
    }
}

#[test]
fn t09_exact_engrave_moves() {
    let mut pr = Project::new();
    let id = add_c(&mut pr, line(p(0.0, 0.0), p(10.0, 0.0)));
    let op = add_op(&mut pr, OpKind::Engrave, vec![id]);
    let tp = run(&pr, op).unwrap();
    let r = |x, y, z| Move::Rapid(Point3 { x, y, z });
    let l = |x, y, z, feed| Move::Linear {
        to: Point3 { x, y, z },
        feed,
    };
    let (z1, z2) = (-5.0 / 3.0, -10.0 / 3.0);
    let want = [
        r(0.0, 0.0, 5.0),
        r(0.0, 0.0, 2.0),
        l(0.0, 0.0, z1, 200.0),
        l(10.0, 0.0, z1, 800.0),
        l(10.0, 0.0, z2, 200.0),
        l(0.0, 0.0, z2, 800.0),
        l(0.0, 0.0, -5.0, 200.0),
        l(10.0, 0.0, -5.0, 800.0),
        r(10.0, 0.0, 5.0),
    ];
    assert_eq!(tp.moves.len(), want.len(), "{:#?}", tp.moves);
    for (i, (a, b)) in tp.moves.iter().zip(&want).enumerate() {
        assert!(same(a, b), "move {i}: {a:?} != {b:?}");
    }
    assert_eq!((tp.op_id, tp.tool_id, tp.rpm), (op, 1, 18000));
}

/// Spec test 10 invariants. Rapids: always end at Z >= clearance; XY rapids also start there
/// (a Z-only retract starts at cutting depth by construction, e.g. test 9's last move).
fn check_invariants(pr: &Project, tp: &Toolpath) {
    let m = &pr.machine;
    let op = pr.operations.iter().find(|o| o.id == tp.op_id).unwrap();
    let Move::Rapid(first) = tp.moves[0] else {
        panic!("first move must be a rapid")
    };
    assert!((first.z - m.safe_z).abs() < T, "first move at safe_z");
    let mut pos = first;
    for mv in &tp.moves[1..] {
        match mv {
            Move::Rapid(to) => {
                let xy = !to.xy().approx_eq(pos.xy(), 1e-9);
                let z = (to.z - pos.z).abs() > 1e-9;
                assert!(!(xy && z), "rapid changes XY and Z: {pos:?} -> {to:?}");
                assert!(to.z >= m.clearance_z - 1e-9, "rapid ends below clearance");
                if xy {
                    assert!(
                        pos.z >= m.clearance_z - 1e-9,
                        "XY rapid starts below clearance"
                    );
                }
                pos = *to;
            }
            Move::Linear { to, feed } => {
                if to.z < pos.z - 1e-9 {
                    assert!(to.xy().approx_eq(pos.xy(), 1e-9), "plunge is vertical");
                    assert!((feed - op.plunge_feed).abs() < T);
                } else if !to.xy().approx_eq(pos.xy(), 1e-9) && to.z < 0.0 {
                    assert!((feed - op.feed).abs() < T);
                    assert!((to.z - pos.z).abs() < 1e-9);
                }
                pos = *to;
            }
            Move::Arc {
                to,
                center,
                ccw,
                feed,
            } => {
                assert!((feed - op.feed).abs() < T);
                assert!((to.z - pos.z).abs() < 1e-9);
                let sweep = arc_sweep(pos.xy(), to.xy(), *center, *ccw);
                assert!(sweep.abs() <= PI + 1e-9, "sweep {sweep}");
                assert!((pos.xy().distance(*center) - to.xy().distance(*center)).abs() < T);
                pos = *to;
            }
            Move::Drill { at, z_bottom, .. } => {
                assert!(at.approx_eq(pos.xy(), 1e-9));
                assert!(*z_bottom >= -op.depth - 1e-9);
            }
        }
        assert!(pos.z >= -op.depth - 1e-9, "below final depth");
    }
    assert!((pos.z - m.safe_z).abs() < T, "ends at safe_z");
}

#[test]
fn t10_invariants_mixed_project() {
    let mut pr = Project::new();
    let sq = add_c(&mut pr, rect(0.0, 0.0, 40.0, 30.0));
    let hole = add_c(&mut pr, circle(20.0, 15.0, 12.0));
    let open = add_c(&mut pr, line(p(50.0, 0.0), p(80.0, 10.0)));
    let big = add_c(&mut pr, circle(150.0, 50.0, 60.0));
    let island = add_c(&mut pr, circle(150.0, 50.0, 20.0));
    let pt = add(&mut pr, Geom::Point(p(100.0, 100.0)));
    let drill_c = add_c(&mut pr, circle(110.0, 100.0, 5.0));
    let kinds = [
        (OpKind::Engrave, vec![sq, open]),
        (contour_kind(Side::Inside, Direction::Climb), vec![hole]),
        (
            contour_kind(Side::Outside, Direction::Conventional),
            vec![sq, hole],
        ),
        (
            contour_kind(Side::On, Direction::Conventional),
            vec![sq, open],
        ),
        (OpKind::Pocket { stepover_pct: 40.0 }, vec![big, island]),
        (OpKind::Drill { peck: Some(1.5) }, vec![pt, drill_c]),
    ];
    for (k, items) in kinds {
        add_op(&mut pr, k, items);
    }
    let all = generate(&pr);
    assert_eq!(all.len(), 6);
    for (id, r) in &all {
        let tp = r.as_ref().unwrap_or_else(|e| panic!("op {id}: {e}"));
        check_invariants(&pr, tp);
    }
}

#[test]
fn t11_drill() {
    let mut pr = Project::new();
    let a = add_c(&mut pr, circle(30.0, 0.0, 5.0));
    let b = add_c(&mut pr, circle(10.0, 0.0, 5.0));
    let c = add(&mut pr, Geom::Point(p(0.0, 0.0)));
    let op = add_op(&mut pr, OpKind::Drill { peck: Some(2.0) }, vec![a, b, c]);
    let o = op_mut(&mut pr, op);
    o.depth = 10.0;
    o.plunge_feed = 100.0;
    let tp = run(&pr, op).unwrap();
    let mut want = Vec::new();
    for x in [0.0, 10.0, 30.0] {
        want.push(Move::Rapid(Point3 { x, y: 0.0, z: 5.0 }));
        want.push(Move::Drill {
            at: p(x, 0.0),
            z_bottom: -10.0,
            peck: Some(2.0),
            r_plane: 2.0,
            feed: 100.0,
        });
    }
    assert_eq!(tp.moves, want);
    let sq = add_c(&mut pr, rect(0.0, 0.0, 5.0, 5.0));
    let bad = add_op(&mut pr, OpKind::Drill { peck: None }, vec![sq]);
    assert_eq!(run(&pr, bad), Err(ToolpathError::UnsupportedItem(sq)));
}

#[test]
fn t12_inner_feature_first() {
    let mut pr = Project::new();
    let sq = add_c(&mut pr, rect(0.0, 0.0, 20.0, 20.0));
    // M10b ruling 1: was Ø5 cut from outside; a hole of the part is now cut from inside (Ø5 < Ø6 would vanish)
    let c = add_c(&mut pr, circle(10.0, 10.0, 10.0));
    let op = add_op(
        &mut pr,
        contour_kind(Side::Outside, Direction::Climb),
        vec![sq, c],
    );
    op_mut(&mut pr, op).depth_per_pass = 5.0;
    let runs = cut_runs(&run(&pr, op).unwrap());
    assert_eq!(runs.len(), 2);
    // the hole's path: Ø10 − 2·3 = Ø4, centred at (10,10)
    let b = runs[0].1.bbox();
    assert!((b.width() - 4.0).abs() < T && b.min.approx_eq(p(8.0, 8.0), T));
    assert!((runs[1].1.bbox().width() - 26.0).abs() < T);
}

/// M10b ruling 1: one Contour op on a square with a hole is one part; the hole is cut from the other side.
#[test]
fn m10b_contour_side_flips_for_holes() {
    use Orientation::{Ccw, Cw};
    let cases = [
        // (side, direction, square: grow, orientation, hole: diameter, orientation)
        (Side::Outside, Direction::Climb, 3.0, Cw, 4.0, Ccw),
        (Side::Inside, Direction::Climb, -3.0, Ccw, 16.0, Cw),
        (Side::Outside, Direction::Conventional, 3.0, Ccw, 4.0, Cw),
        (Side::Inside, Direction::Conventional, -3.0, Cw, 16.0, Ccw),
    ];
    for (side, dir, grow, sq_o, hole_d, hole_o) in cases {
        let mut pr = Project::new();
        let sq = add_c(&mut pr, rect(0.0, 0.0, 50.0, 50.0));
        let hole = add_c(&mut pr, circle(25.0, 25.0, 10.0));
        let op = add_op(&mut pr, contour_kind(side, dir), vec![sq, hole]);
        op_mut(&mut pr, op).depth_per_pass = 5.0;
        let runs = cut_runs(&run(&pr, op).unwrap());
        assert_eq!(runs.len(), 2, "{side:?} {dir:?}");
        // the hole (deeper in the tree) first
        let (h, s) = (&runs[0].1, &runs[1].1);
        let hb = h.bbox();
        assert!((hb.width() - hole_d).abs() < T, "{side:?} {dir:?}");
        assert!(((hb.min + hb.max) * 0.5).approx_eq(p(25.0, 25.0), T));
        assert_eq!(h.orientation(), Some(hole_o), "{side:?} {dir:?} hole");
        let sb = s.bbox();
        assert!(sb.min.approx_eq(p(-grow, -grow), T), "{side:?} {dir:?}");
        assert!(sb.max.approx_eq(p(50.0 + grow, 50.0 + grow), T));
        assert_eq!(s.orientation(), Some(sq_o), "{side:?} {dir:?} square");
    }
}

#[test]
fn t13_errors() {
    let mut pr = Project::new();
    let ln = add_c(&mut pr, line(p(0.0, 0.0), p(10.0, 0.0)));
    let pt = add(&mut pr, Geom::Point(p(1.0, 1.0)));
    let missing_tool = add_op(&mut pr, OpKind::Engrave, vec![ln]);
    op_mut(&mut pr, missing_tool).tool = 9;
    let empty = add_op(&mut pr, OpKind::Engrave, vec![]);
    let pocket_open = add_op(&mut pr, OpKind::Pocket { stepover_pct: 50.0 }, vec![ln]);
    let engrave_pt = add_op(&mut pr, OpKind::Engrave, vec![pt]);
    let missing_item = add_op(&mut pr, OpKind::Engrave, vec![ln, 777]);
    let disabled = add_op(&mut pr, OpKind::Engrave, vec![ln]);
    op_mut(&mut pr, disabled).enabled = false;
    let all = generate(&pr);
    let get = |id| all.iter().find(|(i, _)| *i == id).map(|(_, r)| r.clone());
    assert_eq!(get(missing_tool), Some(Err(ToolpathError::MissingTool(9))));
    assert_eq!(get(empty), Some(Err(ToolpathError::NoItems)));
    assert_eq!(get(pocket_open), Some(Err(ToolpathError::OpenContour(ln))));
    assert_eq!(
        get(engrave_pt),
        Some(Err(ToolpathError::UnsupportedItem(pt)))
    );
    assert_eq!(
        get(missing_item),
        Some(Err(ToolpathError::MissingItem(777)))
    );
    assert_eq!(get(disabled), None);
    assert_eq!(all.len(), 5);
    assert_eq!(
        ToolpathError::OffsetVanished(17).to_string(),
        "Tool too large for item 17"
    );
    assert_eq!(
        ToolpathError::OpenContour(4).to_string(),
        "Needs closed contours (item 4 is open)"
    );
}

fn bracket() -> Project {
    let imp = hc_dxf::import_dxf_file(std::path::Path::new(BRACKET), &Default::default()).unwrap();
    let mut pr = Project::new();
    pr.add_dxf(&imp, "bracket.dxf");
    pr.tools.push(tool(1, 6.35));
    pr
}

/// Ids of Outline circles with diameter `d`.
fn circles(pr: &Project, d: f64) -> Vec<(u64, Circle)> {
    pr.items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            Geom::Contour(c) => c.as_circle().map(|ci| (i.id, ci)),
            _ => None,
        })
        .filter(|(_, c)| (2.0 * c.radius - d).abs() < 1e-6)
        .collect()
}

#[test]
fn t14_bracket() {
    let mut pr = bracket();
    let (outer_id, outer) = pr
        .items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            Geom::Contour(c) if c.closed => Some((i.id, c.clone())),
            _ => None,
        })
        .max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs()))
        .unwrap();
    let r = 6.35 / 2.0;

    let op = add_op(
        &mut pr,
        contour_kind(Side::Outside, Direction::Climb),
        vec![outer_id],
    );
    let o = op_mut(&mut pr, op);
    o.depth = 3.0;
    o.depth_per_pass = 3.0;
    let tp = run(&pr, op).unwrap();
    check_invariants(&pr, &tp);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 1);
    let loop_ = &runs[0].1;
    assert!(loop_.closed, "last cut end = first cut start");
    assert_eq!(loop_.orientation(), Some(Orientation::Cw));
    let (got, want) = (loop_.bbox(), outer.bbox());
    assert!(
        got.min.approx_eq(want.min - p(r, r), T),
        "{got:?} vs {want:?}"
    );
    assert!(
        got.max.approx_eq(want.max + p(r, r), T),
        "{got:?} vs {want:?}"
    );

    let holes = circles(&pr, 15.875);
    assert_eq!(holes.len(), 4);
    let ids: Vec<u64> = holes.iter().map(|h| h.0).collect();
    let inside = add_op(
        &mut pr,
        contour_kind(Side::Inside, Direction::Climb),
        ids.clone(),
    );
    op_mut(&mut pr, inside).depth_per_pass = 5.0;
    let tp = run(&pr, inside).unwrap();
    check_invariants(&pr, &tp);
    let runs = cut_runs(&tp);
    assert_eq!(runs.len(), 4);
    for (_, c) in &runs {
        assert_eq!(c.segments.len(), 2);
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
        for s in &c.segments {
            let Segment::Arc { radius, .. } = *s else {
                panic!("arc expected")
            };
            assert!((radius - (15.875 / 2.0 - r)).abs() < T);
        }
    }

    let big = circles(&pr, 76.2);
    let small = circles(&pr, 44.45);
    assert_eq!((big.len(), small.len()), (1, 1));
    let pocket = add_op(
        &mut pr,
        OpKind::Pocket { stepover_pct: 40.0 },
        vec![big[0].0, small[0].0],
    );
    let tp = run(&pr, pocket).unwrap();
    check_invariants(&pr, &tp);
    let runs = cut_runs(&tp);
    assert!(runs.len() >= 2);
    let c0 = small[0].1.center;
    for (_, c) in &runs {
        for s in &c.segments {
            assert!(
                s.start().distance(c0) >= 22.225 + r - T,
                "ring vertex inside the island"
            );
        }
    }

    let drill = add_op(&mut pr, OpKind::Drill { peck: None }, ids);
    let tp = run(&pr, drill).unwrap();
    check_invariants(&pr, &tp);
    let ats: Vec<Point> = tp
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Drill { at, .. } => Some(*at),
            _ => None,
        })
        .collect();
    assert_eq!(ats.len(), 4);
    for (_, h) in &holes {
        assert!(ats.iter().any(|a| a.approx_eq(h.center, T)));
    }
}

// ---------------------------------------------------------------- M6 (specs/M6.md), tabs

use hc_project::{ManualTabs, TabPlacement, Tabs};

/// R = 100×50 rectangle, CCW, start (0,0), length 300.
fn r_rect() -> Contour {
    rect(0.0, 0.0, 100.0, 50.0)
}

fn tabs_count(count: u32) -> Tabs {
    Tabs {
        enabled: true,
        placement: TabPlacement::Count { count },
        width: 5.0,
        height: 2.0,
        manual: Vec::new(),
    }
}

/// One lifted stretch: up/down vertical at plunge feed, XY path at z_tab.
struct Lift {
    vertical_ok: bool,
    z: f64,
    path: Contour,
    all_linear: bool,
}

impl Lift {
    fn len(&self) -> f64 {
        self.path.length()
    }
    fn mid(&self) -> Point {
        self.path.point_at(self.len() / 2.0)
    }
}

/// (per pass level: z, XY feed length, number of lifts) and every lift. A pass level is set by each
/// downward Linear; moves between a tab's up and down belong to the current level.
fn analyse(tp: &Toolpath, plunge: f64) -> (Vec<(f64, f64, usize)>, Vec<Lift>) {
    let mut levels: Vec<(f64, f64, usize)> = Vec::new();
    let mut lifts: Vec<Lift> = Vec::new();
    let mut lifted = false;
    let mut pos = Point3 {
        x: f64::NAN,
        y: f64::NAN,
        z: f64::NAN,
    };
    for m in &tp.moves {
        let seg = match m {
            Move::Rapid(to) => {
                pos = *to;
                continue;
            }
            Move::Linear { to, feed } => {
                let vertical = to.xy().approx_eq(pos.xy(), 1e-9);
                if to.z < pos.z - 1e-9 {
                    if lifted {
                        let l = lifts.last_mut().unwrap();
                        l.vertical_ok &= vertical && (feed - plunge).abs() < T;
                        lifted = false;
                    } else if levels.last().is_none_or(|l| (l.0 - to.z).abs() > 1e-9) {
                        levels.push((to.z, 0.0, 0));
                    }
                    pos = *to;
                    continue;
                }
                if to.z > pos.z + 1e-9 && to.z <= 1e-9 {
                    lifted = true;
                    levels.last_mut().unwrap().2 += 1;
                    lifts.push(Lift {
                        vertical_ok: vertical && (feed - plunge).abs() < T,
                        z: to.z,
                        path: Contour {
                            segments: Vec::new(),
                            closed: false,
                        },
                        all_linear: true,
                    });
                    pos = *to;
                    continue;
                }
                if vertical {
                    pos = *to;
                    continue;
                }
                Segment::Line {
                    start: pos.xy(),
                    end: to.xy(),
                }
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                let d = pos.xy() - *center;
                if lifted {
                    lifts.last_mut().unwrap().all_linear = false;
                }
                Segment::Arc {
                    center: *center,
                    radius: d.length(),
                    start_angle: d.y.atan2(d.x),
                    sweep: arc_sweep(pos.xy(), to.xy(), *center, *ccw),
                }
            }
            Move::Drill { .. } => continue,
        };
        levels.last_mut().unwrap().1 += seg.length();
        if lifted {
            lifts.last_mut().unwrap().path.segments.push(seg);
        }
        pos = match m {
            Move::Linear { to, .. } | Move::Arc { to, .. } => *to,
            _ => unreachable!(),
        };
    }
    (levels, lifts)
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < T
}

/// Every expected point matches exactly one lift midpoint.
fn assert_mids(lifts: &[Lift], expected: &[(f64, f64)]) {
    assert_eq!(lifts.len(), expected.len());
    for &(x, y) in expected {
        assert!(
            lifts.iter().any(|l| l.mid().approx_eq(p(x, y), T)),
            "no lift at ({x},{y}): {:?}",
            lifts.iter().map(Lift::mid).collect::<Vec<_>>()
        );
    }
}

#[test]
fn m6_01_geom_helpers() {
    let (a, b) = Segment::Line {
        start: p(0.0, 0.0),
        end: p(10.0, 0.0),
    }
    .split_at(4.0);
    assert!(a.start().approx_eq(p(0.0, 0.0), T) && a.end().approx_eq(p(4.0, 0.0), T));
    assert!(b.start().approx_eq(p(4.0, 0.0), T) && b.end().approx_eq(p(10.0, 0.0), T));
    let arc = Segment::Arc {
        center: p(0.0, 0.0),
        radius: 10.0,
        start_angle: 0.0,
        sweep: PI / 2.0,
    };
    let (a, b) = arc.split_at(5.0 * PI / 2.0);
    let (
        Segment::Arc { sweep: s1, .. },
        Segment::Arc {
            sweep: s2,
            start_angle,
            ..
        },
    ) = (a, b)
    else {
        panic!("arcs expected")
    };
    assert!(approx(s1, PI / 4.0) && approx(s2, PI / 4.0) && approx(start_angle, PI / 4.0));
    assert!(a.end().approx_eq(p(7.071068, 7.071068), T));
    let r = r_rect();
    assert!(r.point_at(150.0).approx_eq(p(100.0, 50.0), T));
    assert!(r.point_at(310.0).approx_eq(p(10.0, 0.0), T));
    let (s, d) = r.closest(p(37.5, -3.0));
    assert!(approx(s, 37.5) && approx(d, 3.0));
}

#[test]
fn m6_02_auto_count_and_spacing() {
    let r = r_rect();
    let (pos, note) = tab_positions(&r, &tabs_count(4), 1, 6.0);
    assert_eq!(note, None);
    let want = [0.125, 0.375, 0.625, 0.875];
    assert_eq!(pos.len(), 4);
    for (a, b) in pos.iter().zip(want) {
        assert!(approx(*a, b));
    }
    let pts: Vec<Point> = pos.iter().map(|t| r.point_at(t * 300.0)).collect();
    for (q, (x, y)) in pts
        .iter()
        .zip([(37.5, 0.0), (100.0, 12.5), (62.5, 50.0), (0.0, 37.5)])
    {
        assert!(q.approx_eq(p(x, y), T));
    }
    let spacing = |d| {
        let t = Tabs {
            placement: TabPlacement::Spacing { distance: d },
            ..tabs_count(0)
        };
        tab_positions(&r, &t, 1, 6.0)
    };
    assert_eq!(spacing(80.0).0, pos);
    assert_eq!(spacing(100.0).0.len(), 3);
    assert_eq!(spacing(1000.0).0.len(), 1);
}

#[test]
fn m6_03_snapping() {
    let r = r_rect();
    let (pos, note) = tab_positions(&r, &tabs_count(3), 1, 6.0);
    assert_eq!(note, None);
    let s: Vec<f64> = pos.iter().map(|t| t * 300.0).collect();
    assert_eq!(s.len(), 3);
    for (a, b) in s.iter().zip([50.0, 141.5, 241.5]) {
        assert!(approx(*a, b), "{s:?}");
    }
    for (t, (x, y)) in pos.iter().zip([(50.0, 0.0), (100.0, 41.5), (8.5, 50.0)]) {
        assert!(r.point_at(t * 300.0).approx_eq(p(x, y), T));
    }
}

#[test]
fn m6_04_too_short() {
    let c10 = circle(0.0, 0.0, 10.0);
    let (pos, note) = tab_positions(&c10, &tabs_count(4), 5, 6.0);
    assert_eq!(pos.len(), 1);
    assert_eq!(
        note,
        Some(ToolpathNote::TabsReduced {
            item: 5,
            placed: 1,
            requested: 4
        })
    );
    let c5 = circle(0.0, 0.0, 5.0);
    let (pos, note) = tab_positions(&c5, &tabs_count(4), 5, 6.0);
    assert!(pos.is_empty());
    assert_eq!(
        note,
        Some(ToolpathNote::TabsReduced {
            item: 5,
            placed: 0,
            requested: 4
        })
    );
    for (d, placed) in [(10.0, 1), (5.0, 0)] {
        let mut pr = Project::new();
        let id = add_c(&mut pr, circle(0.0, 0.0, d));
        let op = add_op(
            &mut pr,
            contour_kind(Side::Outside, Direction::Climb),
            vec![id],
        );
        op_mut(&mut pr, op).tabs = tabs_count(4);
        let tp = run(&pr, op).unwrap();
        assert_eq!(
            tp.notes,
            [ToolpathNote::TabsReduced {
                item: id,
                placed,
                requested: 4
            }]
        );
        // lifts in the final pass (depth 5, h 2: the −3.33 pass lifts too)
        assert_eq!(analyse(&tp, 200.0).0.last().unwrap().2, placed);
        check_invariants(&pr, &tp);
    }
}

/// Project with R and one Contour op (depth 6, dpp 2, tabs Count 4 w 5 h 2).
fn r_project(side: Side, d: f64) -> (Project, u64, u64) {
    let mut pr = Project::new();
    pr.tools.push(tool(1, d));
    let id = add_c(&mut pr, r_rect());
    let op = add_op(&mut pr, contour_kind(side, Direction::Climb), vec![id]);
    let o = op_mut(&mut pr, op);
    o.depth = 6.0;
    o.depth_per_pass = 2.0;
    o.tabs = tabs_count(4);
    (pr, id, op)
}

#[test]
fn m6_05_z_profile() {
    let (pr, _, op) = r_project(Side::Outside, 6.0);
    let tp = run(&pr, op).unwrap();
    assert!(tp.notes.is_empty());
    check_invariants(&pr, &tp);
    let (levels, lifts) = analyse(&tp, 200.0);
    assert_eq!(levels.len(), 3);
    for ((z, len, n), (wz, wn)) in levels.iter().zip([(-2.0, 0), (-4.0, 0), (-6.0, 4)]) {
        assert!(approx(*z, wz));
        assert_eq!(*n, wn);
        assert!(approx(*len, 300.0 + 6.0 * PI), "pass {z}: {len}");
    }
    for l in &lifts {
        assert!(l.vertical_ok && l.all_linear);
        assert!(approx(l.z, -4.0));
        assert!(approx(l.len(), 11.0));
    }
    assert_mids(
        &lifts,
        &[(37.5, -3.0), (103.0, 12.5), (62.5, 53.0), (-3.0, 37.5)],
    );
}

#[test]
fn m6_06_inside() {
    let (pr, _, op) = r_project(Side::Inside, 6.0);
    let tp = run(&pr, op).unwrap();
    check_invariants(&pr, &tp);
    let runs = cut_runs(&tp);
    let bb = runs[0].1.bbox();
    assert!(approx(bb.width(), 94.0) && approx(bb.height(), 44.0));
    let (_, lifts) = analyse(&tp, 200.0);
    for l in &lifts {
        assert!(l.vertical_ok && l.all_linear && approx(l.len(), 11.0));
    }
    assert_mids(
        &lifts,
        &[(37.5, 3.0), (97.0, 12.5), (62.5, 47.0), (3.0, 37.5)],
    );
}

#[test]
fn m6_07_circle() {
    let mut pr = Project::new();
    let id = add_c(&mut pr, circle(0.0, 0.0, 60.0));
    let op = add_op(
        &mut pr,
        contour_kind(Side::Outside, Direction::Climb),
        vec![id],
    );
    let o = op_mut(&mut pr, op);
    o.depth = 2.0;
    o.depth_per_pass = 2.0;
    o.tabs = Tabs {
        height: 1.0,
        ..tabs_count(3)
    };
    let tp = run(&pr, op).unwrap();
    check_invariants(&pr, &tp);
    let mut pos = Point::new(f64::NAN, f64::NAN);
    let mut total = 0.0;
    for m in &tp.moves {
        match m {
            Move::Rapid(to) => pos = to.xy(),
            Move::Linear { to, .. } => {
                assert!(to.xy().approx_eq(pos, 1e-9), "XY move that is not an arc");
                pos = to.xy();
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                assert!(center.approx_eq(p(0.0, 0.0), T));
                assert!(
                    approx(pos.distance(*center), 33.0) && approx(to.xy().distance(*center), 33.0)
                );
                total += arc_sweep(pos, to.xy(), *center, *ccw);
                pos = to.xy();
            }
            Move::Drill { .. } => unreachable!(),
        }
    }
    assert!(approx(total.abs(), TAU));
    let (_, lifts) = analyse(&tp, 200.0);
    let lifted: f64 = lifts.iter().map(|l| l.len() / 33.0).sum();
    assert!(approx(lifted, 1.0));
    for l in &lifts {
        assert!(approx(l.len() / 33.0, 11.0 / 33.0));
    }
    let at = |deg: f64| {
        let a = deg.to_radians();
        (33.0 * a.cos(), 33.0 * a.sin())
    };
    assert_mids(&lifts, &[at(60.0), at(180.0), at(300.0)]);
}

#[test]
fn m6_08_loop_start() {
    let (mut pr, id, op) = r_project(Side::Outside, 6.0);
    op_mut(&mut pr, op).tabs.enabled = false;
    let tp = run(&pr, op).unwrap();
    let Move::Rapid(s0) = tp.moves[1] else {
        panic!()
    };
    let s0 = s0.xy();
    let path = cut_runs(&tp)[0].1.clone(); // starts at S0 (s0 = 0)
    let big_p = 300.0 + 6.0 * PI;
    assert!(approx(path.length(), big_p));

    let t = r_rect().closest(s0).0 / 300.0;
    let o = op_mut(&mut pr, op);
    o.tabs.enabled = true;
    o.tabs.manual = vec![ManualTabs {
        item: id,
        positions: vec![t],
    }];
    let tp = run(&pr, op).unwrap();
    check_invariants(&pr, &tp);
    let Move::Rapid(plunge) = tp.moves[1] else {
        panic!()
    };
    let plunge = plunge.xy();
    assert!(
        plunge.approx_eq(path.point_at(big_p / 2.0), T),
        "{plunge:?}"
    );
    let (_, lifts) = analyse(&tp, 200.0);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 11.0));
    assert!(
        lifts[0].mid().approx_eq(s0, T),
        "{:?} vs {s0:?}",
        lifts[0].mid()
    );
    assert!(plunge.distance(s0) > 5.5);
    // closed loop: the last cut end is the plunge XY
    let last_cut = tp
        .moves
        .iter()
        .rev()
        .find_map(|m| match m {
            Move::Linear { to, .. } | Move::Arc { to, .. } if to.z < 0.0 => Some(to.xy()),
            _ => None,
        })
        .unwrap();
    assert!(last_cut.approx_eq(plunge, T));
}

#[test]
fn m6_09_manual_survives_tool_and_side() {
    let (mut pr, id, op) = r_project(Side::Outside, 6.0);
    op_mut(&mut pr, op).tabs.manual = vec![ManualTabs {
        item: id,
        positions: vec![1.0 / 6.0],
    }];
    let lift = |pr: &Project| {
        let tp = run(pr, op).unwrap();
        check_invariants(pr, &tp);
        let (_, l) = analyse(&tp, 200.0);
        assert_eq!(l.len(), 1);
        (l[0].mid(), l[0].len())
    };
    let (m, l) = lift(&pr);
    assert!(m.approx_eq(p(50.0, -3.0), T) && approx(l, 11.0));
    pr.tools[0].diameter = 10.0;
    let (m, l) = lift(&pr);
    assert!(m.approx_eq(p(50.0, -5.0), T) && approx(l, 15.0));
    op_mut(&mut pr, op).kind = contour_kind(Side::Inside, Direction::Climb);
    let (m, _) = lift(&pr);
    assert!(m.approx_eq(p(50.0, 5.0), T));
}

#[test]
fn m6_10_open_other_ops_validation() {
    let mut pr = Project::new();
    let id = add_c(&mut pr, line(p(0.0, 0.0), p(50.0, 0.0)));
    let op = add_op(&mut pr, contour_kind(Side::On, Direction::Climb), vec![id]);
    op_mut(&mut pr, op).tabs = tabs_count(4);
    let tp = run(&pr, op).unwrap();
    assert_eq!(tp.notes, [ToolpathNote::TabsOnOpenContour(id)]);
    assert!(analyse(&tp, 200.0).1.is_empty());
    assert_eq!(
        tp.notes[0].to_string(),
        format!("Item {id} is open: no tabs")
    );

    let sq = add_c(&mut pr, r_rect());
    for kind in [OpKind::Engrave, OpKind::Pocket { stepover_pct: 50.0 }] {
        let op = add_op(&mut pr, kind, vec![sq]);
        let off = run(&pr, op).unwrap();
        op_mut(&mut pr, op).tabs = tabs_count(4);
        let on = run(&pr, op).unwrap();
        assert_eq!(off, on);
        assert!(on.notes.is_empty());
    }

    let (mut pr, _, op) = r_project(Side::Outside, 6.0);
    let e = |pr: &Project| run(pr, op).unwrap_err();
    op_mut(&mut pr, op).tabs.width = 0.0;
    assert_eq!(e(&pr), ToolpathError::InvalidParam("tab_width"));
    op_mut(&mut pr, op).tabs = Tabs {
        height: f64::NAN,
        ..tabs_count(4)
    };
    assert_eq!(e(&pr), ToolpathError::InvalidParam("tab_height"));
    op_mut(&mut pr, op).tabs = Tabs {
        placement: TabPlacement::Spacing { distance: 0.0 },
        ..tabs_count(4)
    };
    assert_eq!(e(&pr), ToolpathError::InvalidParam("tab_spacing"));
    // disabled tabs are not validated
    op_mut(&mut pr, op).tabs.enabled = false;
    assert!(run(&pr, op).is_ok());
}

#[test]
fn m6_11_bracket_outer() {
    let mut pr = bracket();
    let (outer_id, outer) = pr
        .items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            Geom::Contour(c) if c.closed => Some((i.id, c.clone())),
            _ => None,
        })
        .max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs()))
        .unwrap();
    let d = 6.35;
    let tabs = tabs_count(4);
    let (pos, note) = tab_positions(&outer, &tabs, outer_id, d);
    assert_eq!(note, None);
    assert_eq!(pos.len(), 4);
    let m = tabs.width / 2.0 + d;
    let l = outer.length();
    for t in &pos {
        let s = t * l;
        let mut acc = 0.0;
        let seg = outer
            .segments
            .iter()
            .find_map(|seg| {
                let r = (s >= acc && s <= acc + seg.length()).then_some((seg, acc));
                acc += seg.length();
                r
            })
            .unwrap();
        assert!(
            matches!(seg.0, Segment::Line { .. }),
            "tab on an arc at s {s}"
        );
        assert!(s - seg.1 >= m - T && seg.1 + seg.0.length() - s >= m - T);
    }
    let op = add_op(
        &mut pr,
        contour_kind(Side::Outside, Direction::Climb),
        vec![outer_id],
    );
    op_mut(&mut pr, op).tabs = tabs;
    let tp = run(&pr, op).unwrap();
    assert!(tp.notes.is_empty());
    check_invariants(&pr, &tp);
}

#[test]
fn m6_review_r2_z_tab_clamped_to_stock_top() {
    let (mut pr, _, op) = r_project(Side::Outside, 6.0);
    let o = op_mut(&mut pr, op);
    o.depth = 2.0;
    o.depth_per_pass = 2.0;
    o.tabs.height = 3.0;
    let tp = run(&pr, op).unwrap();
    check_invariants(&pr, &tp);
    let (_, lifts) = analyse(&tp, 200.0);
    assert_eq!(lifts.len(), 4);
    for l in &lifts {
        assert!(approx(l.z, 0.0), "lift at {}", l.z);
        assert!(l.vertical_ok && approx(l.len(), 11.0));
    }
    assert!(tp.moves.iter().all(|m| match m {
        Move::Linear { to, .. } | Move::Arc { to, .. } => to.z <= 1e-9,
        _ => true,
    }));
}

fn poly(v: &[(f64, f64)]) -> Contour {
    Contour::from_bulge_vertices(
        &v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect::<Vec<_>>(),
        true,
    )
}

/// Pocket notes of one pocket op on `boundary` (+ islands), Ø6.
fn pocket_notes(boundary: Contour, islands: Vec<Contour>) -> Vec<ToolpathNote> {
    let mut pr = Project::new();
    let mut ids = vec![add_c(&mut pr, boundary)];
    ids.extend(islands.into_iter().map(|c| add_c(&mut pr, c)));
    let op = add_op(&mut pr, OpKind::Pocket { stepover_pct: 0.0 }, ids);
    run(&pr, op).unwrap().notes
}

/// M10b ruling 2: pockets with parts narrower than the tool get a PocketUncut note.
#[test]
fn m10b_pocket_uncut() {
    let r = 3.0;
    let sq = rect(0.0, 0.0, 20.0, 20.0);
    let rounded = hc_geom::shapes::rect(p(0.0, 0.0), 30.0, 20.0, 1.0).unwrap();
    // two 20×20 squares joined by a 2 mm wide, 10 mm long neck
    let neck = poly(&[
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
    ]);
    let hole = circle(10.0, 10.0, 4.0);
    for (name, b, isl, want) in [
        ("square", sq.clone(), vec![], false),
        ("rounded", rounded, vec![], false),
        ("island", sq, vec![hole], false),
        ("neck", neck, vec![], true),
    ] {
        let excess = uncut_excess(&b, &isl, r);
        let notes = pocket_notes(b, isl);
        let got = notes
            .iter()
            .any(|n| matches!(n, ToolpathNote::PocketUncut(1)));
        assert_eq!(got, want, "{name}: excess {excess}");
    }
    assert_eq!(
        ToolpathNote::PocketUncut(7).to_string(),
        "Item 7: parts narrower than the tool"
    );
}

/// M10b ruling 2, spur case: a 20×20 square with a 1×10 mm spur, Ø6 → note.
#[test]
/// Ruling: a 1×15 spur (excess ≈ 11 > r²) warns; a 1×10 spur (≈ 6) is below the r² margin by design.
fn m10b_pocket_uncut_spur() {
    let spur = poly(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 9.5),
        (35.0, 9.5),
        (35.0, 10.5),
        (20.0, 10.5),
        (20.0, 20.0),
        (0.0, 20.0),
    ]);
    let notes = pocket_notes(spur, vec![]);
    assert!(notes
        .iter()
        .any(|n| matches!(n, ToolpathNote::PocketUncut(1))));
}
