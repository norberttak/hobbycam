//! M9 acceptance tests 1–14.

use hc_geom::Point;
use hc_sim::*;
use hc_toolpath::{Move, Point3, Toolpath, ToolpathError};
use std::f64::consts::PI;

const R: f64 = 6000.0;
const T: f64 = 1e-9;

type Paths = Vec<(u64, Result<Toolpath, ToolpathError>)>;

fn tp(op: u64, tool: u32, rpm: u32, moves: Vec<Move>) -> (u64, Result<Toolpath, ToolpathError>) {
    (
        op,
        Ok(Toolpath {
            op_id: op,
            tool_id: tool,
            rpm,
            moves,
            notes: Vec::new(),
        }),
    )
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3 { x, y, z }
}

fn rapid(x: f64, y: f64, z: f64) -> Move {
    Move::Rapid(p(x, y, z))
}

fn lin(x: f64, y: f64, z: f64, feed: f64) -> Move {
    Move::Linear {
        to: p(x, y, z),
        feed,
    }
}

fn arc(x: f64, y: f64, z: f64, ccw: bool) -> Move {
    Move::Arc {
        to: p(x, y, z),
        center: Point::new(0.0, 0.0),
        ccw,
        feed: 600.0,
    }
}

fn drill(x: f64, peck: Option<f64>) -> Move {
    Move::Drill {
        at: Point::new(x, 0.0),
        z_bottom: -5.0,
        peck,
        r_plane: 2.0,
        feed: 100.0,
    }
}

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b}");
}

fn near3(a: Point3, b: Point3, tol: f64) {
    near(a.x, b.x, tol);
    near(a.y, b.y, tol);
    near(a.z, b.z, tol);
}

fn kinds(s: &Sim) -> Vec<StepKind> {
    s.steps.iter().map(|s| s.kind).collect()
}

fn mid(s: &Step) -> Point3 {
    s.point_at((s.t0 + s.t1) / 2.0)
}

fn sim(paths: &Paths) -> Sim {
    simulate(paths, R).unwrap()
}

fn line_job() -> Paths {
    vec![tp(
        1,
        1,
        10000,
        vec![
            rapid(0.0, 0.0, 5.0),
            lin(0.0, 0.0, -1.0, 150.0),
            lin(30.0, 40.0, -1.0, 600.0),
            rapid(30.0, 40.0, 5.0),
        ],
    )]
}

#[test]
fn t1_line() {
    use StepKind::*;
    let s = sim(&line_job());
    assert_eq!(kinds(&s), [Dwell, Feed, Feed, Rapid]);
    near(s.total(), 9.46, T);
    near3(s.position_at(6.9).unwrap(), p(15.0, 20.0, -1.0), T);
    near3(s.position_at(-1.0).unwrap(), p(0.0, 0.0, 5.0), T);
    near3(s.position_at(1e9).unwrap(), p(30.0, 40.0, 5.0), T);
    assert_eq!(s.step_at(1.0), Some(0));
}

fn arc_job(to: Move) -> Sim {
    sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(10.0, 0.0, 5.0), lin(10.0, 0.0, -1.0, 600.0), to],
    )])
}

#[test]
fn t2_arc_ccw() {
    let s = arc_job(arc(0.0, 10.0, -1.0, true));
    let a = s.steps.last().unwrap();
    near(a.t1 - a.t0, 5.0 * PI / 10.0, T);
    let h = 50f64.sqrt();
    near3(mid(a), p(h, h, -1.0), 1e-4);
    assert_eq!(a.point_at(a.t1), a.to);
    assert_eq!(a.to, p(0.0, 10.0, -1.0));
}

#[test]
fn t3_arc_cw() {
    let s = arc_job(arc(0.0, -10.0, -1.0, false));
    let h = 50f64.sqrt();
    near3(mid(s.steps.last().unwrap()), p(h, -h, -1.0), 1e-4);
}

#[test]
fn t4_semicircle() {
    for (ccw, y) in [(true, 10.0), (false, -10.0)] {
        let s = arc_job(arc(-10.0, 0.0, -1.0, ccw));
        let a = s.steps.last().unwrap();
        near3(mid(a), p(0.0, y, -1.0), 1e-9);
        near(a.t1 - a.t0, 10.0 * PI / 10.0, T);
    }
}

#[test]
fn t5_helix() {
    let s = arc_job(arc(0.0, 10.0, -2.0, true));
    let a = s.steps.last().unwrap();
    near(a.t1 - a.t0, (5.0 * PI).hypot(1.0) / 600.0 * 60.0, T);
    near(mid(a).z, -1.5, T);
}

#[test]
fn t6_drill_plain() {
    use StepKind::*;
    let s = sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(0.0, 0.0, 5.0), drill(0.0, None)],
    )]);
    assert_eq!(kinds(&s), [Dwell, Rapid, Feed, Rapid]);
    near(s.steps[1].to.z, 2.0, T);
    near(s.steps[2].to.z, -5.0, T);
    near(s.total(), 6.33, T);
    near3(s.position_at(s.total()).unwrap(), p(0.0, 0.0, 5.0), T);
}

#[test]
fn t7_drill_peck() {
    let s = sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(0.0, 0.0, 5.0), drill(0.0, Some(3.0))],
    )]);
    let feeds: Vec<&Step> = s
        .steps
        .iter()
        .filter(|s| s.kind == StepKind::Feed)
        .collect();
    let bottoms: Vec<f64> = feeds.iter().map(|s| s.to.z).collect();
    assert_eq!(bottoms, [-1.0, -4.0, -5.0]);
    let dur = |k: StepKind| -> f64 {
        s.steps
            .iter()
            .filter(|s| s.kind == k)
            .map(|s| s.t1 - s.t0)
            .sum()
    };
    near(dur(StepKind::Feed), 4.2, T);
    near(dur(StepKind::Rapid), 0.31, T);
    near(s.total(), 6.51, T);
    for q in [1e-9, 0.0] {
        let r = simulate(
            &[tp(4, 1, 1, vec![rapid(0.0, 0.0, 5.0), drill(0.0, Some(q))])],
            R,
        );
        assert_eq!(r, Err(SimError::BadMove { op: 4, index: 1 }));
    }
}

#[test]
fn t8_drill_xy_move() {
    let s = sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(0.0, 0.0, 5.0), drill(10.0, None)],
    )]);
    let r = &s.steps[1];
    assert_eq!(r.kind, StepKind::Rapid);
    assert_eq!((r.from, r.to), (p(0.0, 0.0, 5.0), p(10.0, 0.0, 5.0)));
    near(r.t1 - r.t0, 0.1, T);
}

fn small(op: u64, tool: u32, rpm: u32) -> (u64, Result<Toolpath, ToolpathError>) {
    tp(
        op,
        tool,
        rpm,
        vec![
            rapid(op as f64, 0.0, 5.0),
            lin(op as f64, 0.0, -1.0, 600.0),
            rapid(op as f64, 0.0, 5.0),
        ],
    )
}

#[test]
fn t9_blocks() {
    let s = sim(&vec![
        small(1, 1, 1),
        small(2, 1, 1),
        small(3, 2, 1),
        small(4, 1, 1),
    ]);
    assert_eq!(
        s.blocks.iter().map(|b| b.tool).collect::<Vec<_>>(),
        [1, 2, 1]
    );
    let ops: Vec<Vec<u64>> = s.blocks.iter().map(|b| b.ops.clone()).collect();
    assert_eq!(ops, vec![vec![1, 2], vec![3], vec![4]]);
    assert_eq!(s.blocks[0].t0, 0.0);
    for w in s.blocks.windows(2) {
        assert_eq!(w[1].t0, w[0].t1);
    }
    assert_eq!(s.total(), s.blocks.last().unwrap().t1);
    let blocks: f64 = s.blocks.iter().map(|b| b.t1 - b.t0).sum();
    let steps: f64 = s.steps.iter().map(|s| s.t1 - s.t0).sum();
    near(blocks, steps, T);
}

#[test]
fn t10_spinup() {
    let dwells = |s: &Sim| s.steps.iter().filter(|s| s.kind == StepKind::Dwell).count();
    let s = sim(&vec![small(1, 1, 10000), small(2, 1, 12000)]);
    assert_eq!(dwells(&s), 2);
    let s1 = sim(&vec![small(1, 1, 10000), small(2, 1, 10000)]);
    assert_eq!(dwells(&s1), 1);
    near(s.total() - s1.total(), SPINUP_S, T);
    // op 2's first rapid is timed from (1, 0, 5)
    let r = s1.steps.iter().find(|s| s.op == 2).unwrap();
    assert_eq!(r.kind, StepKind::Rapid);
    assert_eq!((r.from, r.to), (p(1.0, 0.0, 5.0), p(2.0, 0.0, 5.0)));
    // after a tool change: a teleport to the first rapid's target
    let s2 = sim(&vec![small(1, 1, 10000), small(2, 2, 10000)]);
    let first = s2.steps.iter().find(|s| s.op == 2).unwrap();
    assert_eq!(first.kind, StepKind::Dwell);
    assert_eq!(first.from, p(2.0, 0.0, 5.0));
}

#[test]
fn t11_edge_cases() {
    let e = simulate(&[], R).unwrap();
    assert_eq!(e, Sim::default());
    assert_eq!(e.total(), 0.0);
    assert_eq!(e.step_at(0.0), None);
    assert_eq!(e.position_at(1.0), None);

    let err = |id| (id, Err(ToolpathError::NoItems));
    let s = sim(&vec![err(3), err(1)]);
    assert!(s.steps.is_empty());
    assert_eq!(s.skipped, [3, 1]);

    let s = sim(&vec![small(1, 1, 1), err(2), small(3, 1, 1)]);
    assert_eq!(s.skipped, [2]);
    assert_eq!(s.blocks.len(), 1);
    assert_eq!(s.blocks[0].ops, [1, 3]);

    let base = sim(&line_job());
    let mut moves = match &line_job()[0].1 {
        Ok(t) => t.moves.clone(),
        Err(_) => unreachable!(),
    };
    moves.insert(2, lin(0.0, 0.0, -1.0, 150.0));
    moves.insert(2, rapid(0.0, 0.0, -1.0));
    moves.insert(
        2,
        Move::Arc {
            to: p(0.0, 0.0, -1.0),
            center: Point::new(5.0, 0.0),
            ccw: true,
            feed: 600.0,
        },
    );
    let s = sim(&vec![tp(1, 1, 10000, moves)]);
    assert_eq!(s.steps.len(), base.steps.len());
    near(s.total(), base.total(), T);

    let s = sim(&vec![tp(1, 1, 1, vec![rapid(0.0, 0.0, 5.0)])]);
    assert_eq!(s.blocks.len(), 1);
    assert_eq!(kinds(&s), [StepKind::Dwell]);
    near(s.total(), SPINUP_S, T);
}

#[test]
fn t12_bad_input() {
    let bad = |moves: Vec<Move>| simulate(&[tp(7, 1, 1, moves)], R).unwrap_err();
    for f in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            bad(vec![
                rapid(0.0, 0.0, 5.0),
                lin(0.0, 0.0, 0.0, 100.0),
                lin(1.0, 0.0, 0.0, f)
            ]),
            SimError::BadMove { op: 7, index: 2 }
        );
    }
    assert_eq!(
        bad(vec![rapid(0.0, 0.0, 5.0), lin(0.0, 0.0, 5.0, 0.0)]),
        SimError::BadMove { op: 7, index: 1 }
    );
    assert_eq!(
        bad(vec![rapid(0.0, 0.0, 5.0), rapid(f64::NAN, 0.0, 5.0)]),
        SimError::BadMove { op: 7, index: 1 }
    );
    let e = bad(vec![lin(0.0, 0.0, 5.0, 100.0)]);
    assert_eq!(e, SimError::BadMove { op: 7, index: 0 });
    assert_eq!(
        SimError::BadMove { op: 3, index: 7 }.to_string(),
        "Cannot simulate operation 3: move 7 is invalid"
    );
}

#[test]
fn t13_default_rapid() {
    let s = simulate(&line_job(), DEFAULT_RAPID).unwrap();
    let r = s.steps.last().unwrap();
    assert_eq!(r.kind, StepKind::Rapid);
    near(r.t1 - r.t0, 0.144, T);
}

#[test]
#[should_panic]
fn t13_zero_rapid_panics() {
    let _ = simulate(&line_job(), 0.0);
}

#[test]
fn t14_fmt_duration() {
    for (s, want) in [
        (0.0, "0:00"),
        (4.4, "0:04"),
        (4.6, "0:05"),
        (59.5, "1:00"),
        (125.0, "2:05"),
        (3600.0, "1:00:00"),
        (3725.0, "1:02:05"),
        (-3.0, "0:00"),
        (f64::NAN, "0:00"),
    ] {
        assert_eq!(fmt_duration(s), want, "{s}");
    }
}

/// Architect review 5: a zero-sweep arc with a Z change is a straight Feed step.
#[test]
fn zero_sweep_arc_with_dz_is_timed_feed() {
    let s = sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(10.0, 0.0, 0.0), arc(10.0, 0.0, -1.0, true)],
    )]);
    let a = s.steps.last().unwrap();
    assert_eq!((a.kind, a.arc), (StepKind::Feed, None));
    near(a.t1 - a.t0, 0.1, T);
}

/// Architect review C: below R the drill cycle rapids up in Z at the start XY first.
#[test]
fn drill_below_r_plane_retracts_before_xy() {
    let s = sim(&vec![tp(
        1,
        1,
        1,
        vec![rapid(0.0, 0.0, 1.0), drill(10.0, None)],
    )]);
    let r = &s.steps[1];
    assert_eq!(r.kind, StepKind::Rapid);
    assert_eq!((r.from, r.to), (p(0.0, 0.0, 1.0), p(0.0, 0.0, 2.0)));
    assert_eq!(s.steps[2].to, p(10.0, 0.0, 2.0));
    assert_eq!(s.steps.last().unwrap().to, p(10.0, 0.0, 2.0));
}
