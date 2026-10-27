//! M9 edge cases beyond the spec's acceptance tests (verifier).

use hc_geom::Point;
use hc_sim::*;
use hc_toolpath::{Move, Point3, Toolpath, ToolpathError};
use std::f64::consts::PI;

const R: f64 = 6000.0; // 100 mm/s
const T: f64 = 1e-9;

type Entry = (u64, Result<Toolpath, ToolpathError>);

fn tp(op: u64, tool: u32, rpm: u32, moves: Vec<Move>) -> Entry {
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

fn err(op: u64) -> Entry {
    (op, Err(ToolpathError::UnsupportedItem(op)))
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

fn arc_c(to: Point3, c: (f64, f64), ccw: bool) -> Move {
    Move::Arc {
        to,
        center: Point::new(c.0, c.1),
        ccw,
        feed: 600.0,
    }
}

fn drill(z_bottom: f64, peck: Option<f64>, r_plane: f64) -> Move {
    Move::Drill {
        at: Point::new(0.0, 0.0),
        z_bottom,
        peck,
        r_plane,
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

fn sim(paths: &[Entry]) -> Sim {
    simulate(paths, R).unwrap()
}

fn line_job() -> Vec<Entry> {
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

/// Timeline invariants every sim must keep.
fn invariants(s: &Sim) {
    let mut clock = 0.0;
    for st in &s.steps {
        assert_eq!(st.t0, clock);
        assert!(st.t1 >= st.t0 && st.t1.is_finite());
        clock = st.t1;
    }
    assert_eq!(s.total(), clock);
    let mut t = 0.0;
    for b in &s.blocks {
        assert_eq!(b.t0, t);
        t = b.t1;
    }
}

#[test]
fn position_at_time_limits() {
    let s = sim(&line_job());
    let start = p(0.0, 0.0, 5.0);
    let end = p(30.0, 40.0, 5.0);
    assert_eq!(s.position_at(0.0), Some(start));
    assert_eq!(s.position_at(-0.0), Some(start));
    assert_eq!(s.position_at(-1e300), Some(start));
    assert_eq!(s.position_at(f64::NEG_INFINITY), Some(start));
    assert_eq!(s.position_at(s.total()), Some(end));
    assert_eq!(s.position_at(s.total() + 1e-12), Some(end));
    assert_eq!(s.position_at(f64::INFINITY), Some(end));
    assert_eq!(s.step_at(f64::INFINITY), Some(s.steps.len() - 1));
    assert_eq!(s.step_at(-5.0), Some(0));
}

#[test]
fn step_boundaries() {
    let s = sim(&line_job());
    for (i, st) in s.steps.iter().enumerate() {
        // exactly at t1 the step is still active (first step with t1 >= t) and is at its `to`
        assert_eq!(s.step_at(st.t1), Some(i));
        assert_eq!(st.point_at(st.t1), st.to);
        assert_eq!(st.point_at(st.t0), st.from);
        if i + 1 < s.steps.len() {
            assert_eq!(s.step_at(st.t1 + 1e-9), Some(i + 1));
            // continuous across the boundary
            near3(s.position_at(st.t1 + 1e-12).unwrap(), st.to, 1e-6);
        }
    }
    // the Dwell holds the block-start position
    near3(s.position_at(1.0).unwrap(), p(0.0, 0.0, 5.0), T);
}

fn arc_sim(from: Point3, to: Point3, c: (f64, f64), ccw: bool) -> Sim {
    sim(&[tp(
        1,
        1,
        10000,
        vec![
            Move::Rapid(from),
            arc_c(to, c, ccw),
            Move::Rapid(Point3 { z: 5.0, ..to }),
        ],
    )])
}

/// Every sample of the arc lies on its radius, and the angle moves monotonically in the right direction.
fn check_on_circle(st: &Step, c: Point, r: f64, ccw: bool) {
    let mut prev = None::<f64>;
    for k in 0..=64 {
        let t = st.t0 + (st.t1 - st.t0) * k as f64 / 64.0;
        let q = st.point_at(t);
        near((q.xy() - c).length(), r, 1e-9);
        let a = (q.y - c.y).atan2(q.x - c.x);
        if let Some(pa) = prev {
            let mut d = a - pa;
            if d > PI {
                d -= 2.0 * PI;
            }
            if d < -PI {
                d += 2.0 * PI;
            }
            assert!(if ccw { d > 0.0 } else { d < 0.0 }, "k {k}: d {d}");
        }
        prev = Some(a);
    }
}

#[test]
fn near_pi_arcs_stay_on_radius() {
    // offset centre so the atan2 seam is crossed for some directions
    let c = (3.0, -2.0);
    let r = 7.0;
    for &ccw in &[true, false] {
        for &a0 in &[0.0, 1.0, PI - 0.1, -PI + 0.05, 2.5] {
            let sweep = if ccw { PI - 1e-6 } else { -(PI - 1e-6) };
            let from = p(c.0 + r * f64::cos(a0), c.1 + r * f64::sin(a0), -1.0);
            let a1 = a0 + sweep;
            let to = p(c.0 + r * a1.cos(), c.1 + r * a1.sin(), -1.0);
            let s = arc_sim(from, to, c, ccw);
            let st = s.steps.iter().find(|s| s.arc.is_some()).unwrap();
            let (_, sw) = st.arc.unwrap();
            near(sw, sweep, 1e-9);
            near(st.t1 - st.t0, r * sweep.abs() / 10.0, 1e-9);
            check_on_circle(st, Point::new(c.0, c.1), r, ccw);
            invariants(&s);
        }
    }
}

#[test]
fn semicircle_cw_ccw_on_radius() {
    for &ccw in &[true, false] {
        let s = arc_sim(p(10.0, 0.0, -1.0), p(-10.0, 0.0, -1.0), (0.0, 0.0), ccw);
        let st = s.steps.iter().find(|s| s.arc.is_some()).unwrap();
        near(st.arc.unwrap().1.abs(), PI, 1e-12);
        check_on_circle(st, Point::new(0.0, 0.0), 10.0, ccw);
    }
}

#[test]
fn long_way_arc_is_timed_as_drawn() {
    // generate never emits |sweep| > π, but a ccw (10,0) → (0,-10) is 3π/2 the long way
    let s = arc_sim(p(10.0, 0.0, -1.0), p(0.0, -10.0, -1.0), (0.0, 0.0), true);
    let st = s.steps.iter().find(|s| s.arc.is_some()).unwrap();
    near(st.arc.unwrap().1, 1.5 * PI, 1e-12);
    near(st.t1 - st.t0, 15.0 * PI / 10.0, 1e-9);
    check_on_circle(st, Point::new(0.0, 0.0), 10.0, true);
    near3(
        st.point_at((st.t0 + st.t1) / 2.0),
        p(-7.0710678, 7.0710678, -1.0),
        1e-6,
    );
}

#[test]
fn helix_z_linear_in_time() {
    let s = arc_sim(p(10.0, 0.0, 0.0), p(0.0, 10.0, -4.0), (0.0, 0.0), true);
    let st = s.steps.iter().find(|s| s.arc.is_some()).unwrap();
    near(st.t1 - st.t0, (5.0 * PI).hypot(4.0) / 10.0, T);
    for k in 0..=8 {
        let f = k as f64 / 8.0;
        let q = st.point_at(st.t0 + f * (st.t1 - st.t0));
        near(q.z, -4.0 * f, 1e-9);
        near((q.xy() - Point::new(0.0, 0.0)).length(), 10.0, 1e-9);
    }
}

#[test]
fn arc_radius_change_is_linear() {
    // end radius 12 (not on the start circle): radius lerps 10 → 12
    let s = arc_sim(p(10.0, 0.0, -1.0), p(0.0, 12.0, -1.0), (0.0, 0.0), true);
    let st = s.steps.iter().find(|s| s.arc.is_some()).unwrap();
    let m = st.point_at((st.t0 + st.t1) / 2.0);
    near((m.xy() - Point::new(0.0, 0.0)).length(), 11.0, 1e-9);
    // length uses r0 (spec)
    near(st.t1 - st.t0, 5.0 * PI / 10.0, T);
}

#[test]
fn arc_from_centre_with_zero_length_is_dropped() {
    // start == centre, to == centre: zero radius and no Z change → no step, no NaN
    let s = arc_sim(p(0.0, 0.0, -1.0), p(0.0, 0.0, -1.0), (0.0, 0.0), true);
    assert!(s.steps.iter().all(|s| s.arc.is_none()));
    invariants(&s);
}

#[test]
fn zero_sweep_arc_with_z_change_is_a_straight_feed() {
    // architect review 5: the post writes it as G1, so it is a timed straight Feed step
    let s = arc_sim(p(10.0, 0.0, 0.0), p(10.0, 0.0, -2.0), (0.0, 0.0), true);
    assert!(s.steps.iter().all(|s| s.arc.is_none()));
    let f = s.steps.iter().find(|s| s.kind == StepKind::Feed).unwrap();
    assert_eq!((f.from, f.to), (p(10.0, 0.0, 0.0), p(10.0, 0.0, -2.0)));
    near(f.t1 - f.t0, 2.0 / 600.0 * 60.0, T);
    invariants(&s);
}

fn drill_sim(z0: f64, m: Move) -> Sim {
    sim(&[tp(1, 1, 10000, vec![rapid(0.0, 0.0, z0), m])])
}

#[test]
fn peck_uneven_last_peck() {
    // r 2, bottom -5, q 3.5 → n 2, bottoms -1.5, -5
    // feed 3.5 + 3.5 = 7 mm → 4.2 s; rapids 3 (5→2) + 3.5 (up) + 3.5 (down to -1.5) + 10 (to 5) = 20 mm → 0.2 s
    let s = drill_sim(5.0, drill(-5.0, Some(3.5), 2.0));
    let feeds: Vec<f64> = s
        .steps
        .iter()
        .filter(|s| s.kind == StepKind::Feed)
        .map(|s| s.to.z)
        .collect();
    assert_eq!(feeds, [-1.5, -5.0]);
    near(s.total(), 2.0 + 4.2 + 0.2, T);
    assert_eq!(s.steps.last().unwrap().to, p(0.0, 0.0, 5.0));
    invariants(&s);
}

#[test]
fn peck_count_rounding_tolerance() {
    // 7 / (7/3) may be 3.0000000000000004: must still be 3 pecks, not 4
    let s = drill_sim(5.0, drill(-5.0, Some(7.0 / 3.0), 2.0));
    let n = s.steps.iter().filter(|s| s.kind == StepKind::Feed).count();
    assert_eq!(n, 3);
    // feed 7 mm (4.2 s); rapid 3 + (7/3 up, 7/3 down) + (14/3 up, 14/3 down) + 10 = 27 mm
    near(s.total(), 2.0 + 4.2 + 0.27, T);
}

#[test]
fn peck_larger_than_depth_is_one_feed() {
    let s = drill_sim(5.0, drill(-5.0, Some(50.0), 2.0));
    let plain = drill_sim(5.0, drill(-5.0, None, 2.0));
    assert_eq!(s.steps, plain.steps);
    near(s.total(), 6.33, T);
}

#[test]
fn drill_starting_below_r_plane() {
    // G98 with the start Z below R retracts to R (LinuxCNC), position afterwards (at, r)
    let s = drill_sim(1.0, drill(-5.0, None, 2.0));
    // rapid 1 → 2 (1 mm), feed 7 mm, rapid back to 2 (7 mm)
    near(s.total(), 2.0 + 0.01 + 4.2 + 0.07, T);
    assert_eq!(s.steps.last().unwrap().to, p(0.0, 0.0, 2.0));
}

#[test]
fn drill_starting_at_r_plane() {
    // no zero-length rapid to R
    let s = drill_sim(2.0, drill(-5.0, None, 2.0));
    let kinds: Vec<StepKind> = s.steps.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, [StepKind::Dwell, StepKind::Feed, StepKind::Rapid]);
    near(s.total(), 2.0 + 4.2 + 0.07, T);
}

#[test]
fn max_pecks_boundary() {
    // r 0, bottom -10, q 0.001 → 10_000 pecks: allowed
    let s = drill_sim(0.0, drill(-10.0, Some(0.001), 0.0));
    let n = s.steps.iter().filter(|s| s.kind == StepKind::Feed).count();
    assert_eq!(n, MAX_PECKS);
    invariants(&s);
    // one more peck → BadMove
    let r = simulate(
        &[tp(
            7,
            1,
            1,
            vec![rapid(0.0, 0.0, 0.0), drill(-10.001, Some(0.001), 0.0)],
        )],
        R,
    );
    assert_eq!(r, Err(SimError::BadMove { op: 7, index: 1 }));
}

#[test]
fn drill_bad_values() {
    let bad = [
        drill(-5.0, Some(-1.0), 2.0),
        drill(-5.0, Some(f64::NAN), 2.0),
        drill(-5.0, Some(f64::INFINITY), 2.0),
        drill(f64::NAN, None, 2.0),
        drill(-5.0, None, f64::INFINITY),
        Move::Drill {
            at: Point::new(f64::NAN, 0.0),
            z_bottom: -5.0,
            peck: None,
            r_plane: 2.0,
            feed: 100.0,
        },
        Move::Drill {
            at: Point::new(0.0, 0.0),
            z_bottom: -5.0,
            peck: None,
            r_plane: 2.0,
            feed: 0.0,
        },
    ];
    for m in bad {
        let r = simulate(&[tp(3, 1, 1, vec![rapid(0.0, 0.0, 5.0), m.clone()])], R);
        assert_eq!(r, Err(SimError::BadMove { op: 3, index: 1 }), "{m:?}");
    }
}

#[test]
fn arc_bad_values() {
    let bad = [
        Move::Arc {
            to: p(0.0, 10.0, 0.0),
            center: Point::new(f64::NAN, 0.0),
            ccw: true,
            feed: 600.0,
        },
        Move::Arc {
            to: p(0.0, 10.0, 0.0),
            center: Point::new(0.0, 0.0),
            ccw: true,
            feed: f64::INFINITY,
        },
        Move::Arc {
            to: p(0.0, 10.0, 0.0),
            center: Point::new(0.0, 0.0),
            ccw: true,
            feed: -600.0,
        },
        lin(0.0, 0.0, 0.0, f64::INFINITY),
        rapid(f64::INFINITY, 0.0, 0.0),
    ];
    for m in bad {
        let r = simulate(&[tp(3, 1, 1, vec![rapid(10.0, 0.0, 0.0), m.clone()])], R);
        assert_eq!(r, Err(SimError::BadMove { op: 3, index: 1 }), "{m:?}");
    }
}

#[test]
fn bad_move_in_later_op_fails_whole_sim() {
    let r = simulate(
        &[
            line_job().remove(0),
            err(2),
            tp(3, 1, 1, vec![rapid(0.0, 0.0, 5.0), lin(1.0, 0.0, 5.0, 0.0)]),
        ],
        R,
    );
    assert_eq!(r, Err(SimError::BadMove { op: 3, index: 1 }));
    // an empty move list has no first Rapid
    let r = simulate(&[tp(4, 1, 1, vec![])], R);
    assert_eq!(r, Err(SimError::BadMove { op: 4, index: 0 }));
}

fn small(op: u64, tool: u32, rpm: u32, x: f64) -> Entry {
    // rapid to (x,0,5), plunge 6 mm at 360 (1 s), cut 10 mm at 600 (1 s), retract 6 mm (0.06 s)
    tp(
        op,
        tool,
        rpm,
        vec![
            rapid(x, 0.0, 5.0),
            lin(x, 0.0, -1.0, 360.0),
            lin(x + 10.0, 0.0, -1.0, 600.0),
            rapid(x + 10.0, 0.0, 5.0),
        ],
    )
}

#[test]
fn multi_block_timing() {
    // T1 a, T1 b (same rpm), T2 c, T1 d (new rpm)
    let s = sim(&[
        small(1, 1, 10000, 0.0),
        small(2, 1, 10000, 20.0),
        small(3, 2, 3000, 0.0),
        small(4, 1, 12000, 0.0),
    ]);
    invariants(&s);
    assert_eq!(s.blocks.len(), 3);
    let op = 2.06;
    // block 1: dwell + a + rapid (10,0,5)→(20,0,5) 0.1 s + b
    near(s.blocks[0].t1 - s.blocks[0].t0, 2.0 + op + 0.1 + op, T);
    near(s.blocks[1].t1 - s.blocks[1].t0, 2.0 + op, T);
    near(s.blocks[2].t1 - s.blocks[2].t0, 2.0 + op, T);
    let sum: f64 = s.blocks.iter().map(|b| b.t1 - b.t0).sum();
    near(sum, s.total(), T);
    near(s.total(), 6.0 + 4.0 * op + 0.1, T);
    // every step lies inside the block of its tool
    for st in &s.steps {
        let b = s
            .blocks
            .iter()
            .find(|b| b.t0 <= st.t0 && st.t1 <= b.t1 && b.ops.contains(&st.op))
            .unwrap();
        assert_eq!(b.tool, st.tool);
    }
}

#[test]
fn rpm_change_dwell_position_and_rpm() {
    let s = sim(&[
        small(1, 1, 10000, 0.0),
        small(2, 1, 12000, 20.0),
        small(3, 1, 10000, 40.0),
        small(4, 1, 10000, 60.0),
    ]);
    let dwells: Vec<&Step> = s
        .steps
        .iter()
        .filter(|s| s.kind == StepKind::Dwell)
        .collect();
    // start, 10000→12000, 12000→10000; op 4 has the same rpm as op 3
    assert_eq!(dwells.len(), 3);
    assert_eq!(
        dwells.iter().map(|d| (d.op, d.rpm)).collect::<Vec<_>>(),
        [(1, 10000), (2, 12000), (3, 10000)]
    );
    // the dwell happens where the previous op ended, before the rapid to the next op
    assert_eq!(dwells[1].from, p(10.0, 0.0, 5.0));
    assert_eq!(dwells[1].to, dwells[1].from);
    near(dwells[1].t1 - dwells[1].t0, SPINUP_S, 0.0);
    assert_eq!(s.blocks.len(), 1);
    invariants(&s);
}

#[test]
fn rpm_zero_first_op_still_dwells() {
    // prev_rpm starts at 0: a block start always dwells regardless of rpm
    let s = sim(&[small(1, 1, 0, 0.0), small(2, 1, 0, 20.0)]);
    let n = s.steps.iter().filter(|s| s.kind == StepKind::Dwell).count();
    assert_eq!(n, 1);
}

#[test]
fn skipped_ops_positions() {
    let s = sim(&[
        err(1),
        small(2, 1, 10000, 0.0),
        err(3),
        small(4, 2, 10000, 0.0),
        err(5),
    ]);
    assert_eq!(s.skipped, [1, 3, 5]);
    assert_eq!(s.blocks.len(), 2);
    assert_eq!(s.blocks[0].ops, [2]);
    assert_eq!(s.blocks[1].ops, [4]);
    assert!(s.steps.iter().all(|st| st.op == 2 || st.op == 4));
    invariants(&s);
    // an Err between two ops of the same tool and rpm: still one block and one dwell
    let s = sim(&[small(1, 1, 9000, 0.0), err(2), small(3, 1, 9000, 20.0)]);
    assert_eq!(s.blocks.len(), 1);
    assert_eq!(s.blocks[0].ops, [1, 3]);
    let n = s.steps.iter().filter(|s| s.kind == StepKind::Dwell).count();
    assert_eq!(n, 1);
}

#[test]
fn tiny_feed_gives_finite_large_time() {
    let s = sim(&[tp(
        1,
        1,
        1,
        vec![rapid(0.0, 0.0, 0.0), lin(10.0, 0.0, 0.0, 1e-6)],
    )]);
    near(s.total(), 2.0 + 10.0 / 1e-6 * 60.0, 1e-3);
    assert!(s.total().is_finite());
    let st = s.steps.last().unwrap();
    near3(st.point_at((st.t0 + st.t1) / 2.0), p(5.0, 0.0, 0.0), 1e-9);
}

#[test]
fn extreme_finite_values_never_give_infinite_time() {
    for m in [
        lin(10.0, 0.0, 0.0, f64::MIN_POSITIVE),
        lin(1e200, 1e200, 0.0, 600.0),
    ] {
        let r = simulate(&[tp(1, 1, 1, vec![rapid(0.0, 0.0, 0.0), m.clone()])], R);
        match r {
            Ok(s) => assert!(s.total().is_finite(), "{m:?}: total {}", s.total()),
            Err(e) => assert_eq!(e, SimError::BadMove { op: 1, index: 1 }),
        }
    }
}

#[test]
fn deterministic() {
    let paths = [
        small(1, 1, 10000, 0.0),
        tp(
            2,
            2,
            3000,
            vec![rapid(0.0, 0.0, 5.0), drill(-5.0, Some(1.3), 2.0)],
        ),
    ];
    assert_eq!(simulate(&paths, R), simulate(&paths, R));
}

#[test]
fn rapid_rate_scales_rapids_only() {
    let a = sim(&line_job());
    let b = simulate(&line_job(), 2.0 * R).unwrap();
    // only the one 6 mm rapid (0.06 s) halves
    near(a.total() - b.total(), 0.03, T);
}

#[test]
#[should_panic]
fn nan_rapid_panics() {
    let _ = simulate(&line_job(), f64::NAN);
}

#[test]
#[should_panic]
fn negative_rapid_panics() {
    let _ = simulate(&line_job(), -1.0);
}

#[test]
fn fmt_duration_extra() {
    assert_eq!(fmt_duration(0.49), "0:00");
    assert_eq!(fmt_duration(3599.5), "1:00:00");
    assert_eq!(fmt_duration(36000.0), "10:00:00");
    assert_eq!(fmt_duration(f64::NEG_INFINITY), "0:00");
}
