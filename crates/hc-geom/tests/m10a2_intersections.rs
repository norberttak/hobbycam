//! M10a.2 verifier: Segment::intersections edge cases.

use hc_geom::{Point, Segment};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Segment {
    Segment::Line {
        start: p(x0, y0),
        end: p(x1, y1),
    }
}

fn arc(cx: f64, cy: f64, r: f64, start: f64, sweep: f64) -> Segment {
    Segment::Arc {
        center: p(cx, cy),
        radius: r,
        start_angle: start,
        sweep,
    }
}

/// Both orders give `want` (within tol), and every point lies on both segments.
#[track_caller]
fn check(a: Segment, b: Segment, want: &[(f64, f64)], tol: f64) {
    for (x, y) in [(a, b), (b, a)] {
        let got = x.intersections(&y);
        assert_eq!(got.len(), want.len(), "{x:?} x {y:?}: {got:?}");
        for &(wx, wy) in want {
            assert!(
                got.iter().any(|g| g.distance(p(wx, wy)) <= tol),
                "{got:?} lacks ({wx}, {wy})"
            );
        }
        for g in &got {
            assert!(
                x.distance_to(*g) <= tol.max(1e-9) && y.distance_to(*g) <= tol.max(1e-9),
                "{g:?} not on both: {} {}",
                x.distance_to(*g),
                y.distance_to(*g)
            );
        }
    }
}

#[test]
fn near_parallel_lines_still_cross() {
    // 1e-6 rad apart, crossing at (5,0)
    let a = 1e-6f64;
    check(
        line(0.0, 0.0, 10.0, 0.0),
        line(0.0, -5.0 * a.tan(), 10.0, 5.0 * a.tan()),
        &[(5.0, 0.0)],
        1e-9,
    );
    // parallel within 1e-12 rad and 1 µm apart: nothing (no far-away garbage point)
    let got = line(0.0, 0.0, 10.0, 0.0).intersections(&line(0.0, 1e-3, 10.0, 1e-3 + 1e-11));
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn very_short_segments() {
    // a 0.1 µm line crossing a long one
    check(
        line(5.0, -5e-5, 5.0, 5e-5),
        line(0.0, 0.0, 10.0, 0.0),
        &[(5.0, 0.0)],
        1e-9,
    );
    // a zero-length line gives nothing (and does not panic)
    assert!(line(5.0, 0.0, 5.0, 0.0)
        .intersections(&line(0.0, 0.0, 10.0, 0.0))
        .is_empty());
    assert!(line(5.0, 0.0, 5.0, 0.0)
        .intersections(&arc(0.0, 0.0, 5.0, 0.0, TAU))
        .is_empty());
    // a tiny arc (1e-3 rad of r 1) crossed by a line through its middle
    let s = arc(0.0, 0.0, 1.0, -5e-4, 1e-3);
    check(line(0.5, 0.0, 1.5, 0.0), s, &[(1.0, 0.0)], 1e-9);
}

#[test]
fn arcs_across_the_seam() {
    // CCW arc from 3π/2 over 0 to π/2 (right half), given as start 3π/2 and as start −π/2
    for start in [1.5 * PI, -FRAC_PI_2, 1.5 * PI + TAU] {
        let a = arc(0.0, 0.0, 5.0, start, PI);
        check(line(-10.0, 0.0, 10.0, 0.0), a, &[(5.0, 0.0)], 1e-9);
        check(line(-10.0, 3.0, 10.0, 3.0), a, &[(4.0, 3.0)], 1e-9);
        check(line(-10.0, -3.0, 10.0, -3.0), a, &[(4.0, -3.0)], 1e-9);
    }
    // the same span as a CW arc from π/2 to −π/2
    let cw = arc(0.0, 0.0, 5.0, FRAC_PI_2, -PI);
    check(line(-10.0, 0.0, 10.0, 0.0), cw, &[(5.0, 0.0)], 1e-9);
    // the complementary CW arc (left half) does not see (5,0)
    let left = arc(0.0, 0.0, 5.0, -FRAC_PI_2, -PI);
    check(line(-10.0, 0.0, 10.0, 0.0), left, &[(-5.0, 0.0)], 1e-9);
}

#[test]
fn arc_end_points_are_inclusive_and_just_outside_excluded() {
    let q = arc(0.0, 0.0, 5.0, 0.0, FRAC_PI_2);
    // line through the arc end (0,5)
    check(line(-10.0, 5.0, 10.0, 5.0), q, &[(0.0, 5.0)], 1e-9);
    // a line hitting the circle 1e-6 rad beyond the arc start: excluded
    let t = -1e-6f64;
    let (x, y) = (5.0 * t.cos(), 5.0 * t.sin());
    let got = line(x, y - 1.0, x, y + 1e-9).intersections(&q);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn arc_arc_cw_ccw_and_concentric() {
    // upper half CCW and the same as CW give the same answer against a circle at (8,0)
    let other = arc(8.0, 0.0, 5.0, 0.0, TAU);
    check(arc(0.0, 0.0, 5.0, 0.0, PI), other, &[(4.0, 3.0)], 1e-9);
    check(arc(0.0, 0.0, 5.0, PI, -PI), other, &[(4.0, 3.0)], 1e-9);
    // concentric partial arcs, coincident arcs, overlapping spans: nothing
    check(
        arc(0.0, 0.0, 5.0, 0.0, PI),
        arc(0.0, 0.0, 5.0, FRAC_PI_2, PI),
        &[],
        1e-9,
    );
    check(
        arc(0.0, 0.0, 5.0, 0.0, PI),
        arc(0.0, 0.0, 3.0, 0.0, PI),
        &[],
        1e-9,
    );
    // two arcs meeting end to end on the same circle: nothing (coincident circle), their ends are Corners anyway
    check(
        arc(0.0, 0.0, 5.0, 0.0, PI),
        arc(0.0, 0.0, 5.0, PI, PI),
        &[],
        1e-9,
    );
    // equal radii crossing arcs where only one of the two circle points is on both spans
    check(
        arc(0.0, 0.0, 5.0, -FRAC_PI_2, PI),
        arc(8.0, 0.0, 5.0, FRAC_PI_2, PI),
        &[(4.0, 3.0), (4.0, -3.0)],
        1e-9,
    );
    check(
        arc(0.0, 0.0, 5.0, 0.0, FRAC_PI_2),
        arc(8.0, 0.0, 5.0, FRAC_PI_2, PI),
        &[(4.0, 3.0)],
        1e-9,
    );
}

#[test]
fn tangent_tolerance_boundaries() {
    let c = arc(0.0, 0.0, 5.0, 0.0, TAU);
    // tangent within EPS: one point
    check(
        line(-10.0, 5.0 + 5e-10, 10.0, 5.0 + 5e-10),
        c,
        &[(0.0, 5.0)],
        1e-9,
    );
    // 1e-6 outside: nothing
    check(line(-10.0, 5.0 + 1e-6, 10.0, 5.0 + 1e-6), c, &[], 1e-9);
    // 1e-6 inside: two real points ~6.3e-3 apart, both on the circle
    let got = line(-10.0, 5.0 - 1e-6, 10.0, 5.0 - 1e-6).intersections(&c);
    assert_eq!(got.len(), 2, "{got:?}");
    // circles tangent within EPS (external and internal)
    check(
        c,
        arc(10.0 + 5e-10, 0.0, 5.0, 0.0, TAU),
        &[(5.0, 0.0)],
        1e-9,
    );
    check(c, arc(3.0 - 5e-10, 0.0, 2.0, 0.0, TAU), &[(5.0, 0.0)], 1e-9);
    // internal tangency with the small circle first
    check(arc(-3.0, 0.0, 2.0, 0.0, TAU), c, &[(-5.0, 0.0)], 1e-9);
}

#[test]
fn inch_scale_coordinates() {
    let i = 25.4;
    // a 1/8 in hole crossed by a line through its centre
    let c = arc(1.5 * i, 2.25 * i, 0.0625 * i, 0.0, TAU);
    check(
        line(0.0, 2.25 * i, 4.0 * i, 2.25 * i),
        c,
        &[(1.4375 * i, 2.25 * i), (1.5625 * i, 2.25 * i)],
        1e-9,
    );
    check(
        line(0.1 * i, 0.1 * i, 3.9 * i, 3.9 * i),
        line(0.1 * i, 3.9 * i, 3.9 * i, 0.1 * i),
        &[(2.0 * i, 2.0 * i)],
        1e-9,
    );
}

#[test]
fn large_coordinates() {
    let o = 1e5;
    check(
        line(o, o, o + 10.0, o + 10.0),
        line(o, o + 10.0, o + 10.0, o),
        &[(o + 5.0, o + 5.0)],
        1e-8,
    );
    let c = arc(o, -o, 5.0, 0.0, TAU);
    check(
        line(o - 10.0, -o, o + 10.0, -o),
        c,
        &[(o - 5.0, -o), (o + 5.0, -o)],
        1e-8,
    );
    check(
        line(o - 10.0, -o + 5.0, o + 10.0, -o + 5.0),
        c,
        &[(o, -o + 5.0)],
        1e-8,
    );
    check(
        c,
        arc(o + 8.0, -o, 5.0, 0.0, TAU),
        &[(o + 4.0, -o + 3.0), (o + 4.0, -o - 3.0)],
        1e-8,
    );
    check(c, arc(o + 10.0, -o, 5.0, 0.0, TAU), &[(o + 5.0, -o)], 1e-8);
}

#[test]
fn closest_on_a_1_micron_line() {
    let l = line(5.0, -5e-7, 5.0, 5e-7);
    assert!(
        l.distance_to(p(5.0, 0.0)) < 1e-12,
        "{}",
        l.distance_to(p(5.0, 0.0))
    );
}
