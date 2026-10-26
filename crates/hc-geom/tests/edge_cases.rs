//! Tester edge cases for M1 (public API only).

use hc_geom::*;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, PI, TAU};

const T: f64 = 1e-9;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Segment {
    Segment::Line {
        start: p(x0, y0),
        end: p(x1, y1),
    }
}

fn arc(cx: f64, cy: f64, r: f64, start_angle: f64, sweep: f64) -> Segment {
    Segment::Arc {
        center: p(cx, cy),
        radius: r,
        start_angle,
        sweep,
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

fn polygon(pts: &[(f64, f64)]) -> Contour {
    let v: Vec<(Point, f64)> = pts.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
    Contour::from_bulge_vertices(&v, true)
}

fn square(cx: f64, cy: f64, s: f64) -> Contour {
    let h = s / 2.0;
    polygon(&[
        (cx - h, cy - h),
        (cx + h, cy - h),
        (cx + h, cy + h),
        (cx - h, cy + h),
    ])
}

fn circle(x: f64, y: f64, r: f64) -> Contour {
    Circle {
        center: p(x, y),
        radius: r,
    }
    .to_contour()
}

fn assert_bbox(b: BBox, min: (f64, f64), max: (f64, f64), tol: f64) {
    assert!(
        b.min.approx_eq(p(min.0, min.1), tol) && b.max.approx_eq(p(max.0, max.1), tol),
        "bbox {b:?} != {min:?}..{max:?}"
    );
}

// ---------- chaining ----------

#[test]
fn chain_drops_zero_length_line_and_zero_arcs() {
    let mut input = vec![
        line(0.0, 0.0, 1.0, 0.0),
        line(1.0, 0.0, 1.0, 0.0),         // zero-length line
        arc(1.0, 0.0, 0.0, 0.0, PI),      // zero radius arc
        line(1.0, 0.0, 1.0, 1.0 + 1e-12), // fine
        line(1.0, 1.0, 1.0 + 1e-10, 1.0), // shorter than EPS
        line(1.0, 1.0, 0.0, 1.0),
        line(0.0, 1.0, 0.0, 0.0),
    ];
    let out = chain_segments(&input, 0.01);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].segments.len(), 4);
    assert!(out[0].closed);
    assert!(close(out[0].signed_area().abs(), 1.0, 1e-9));

    // only degenerate segments -> nothing
    input.retain(|s| s.length() < EPS);
    assert!(chain_segments(&input, 0.01).is_empty());
    assert!(chain_segments(&[], 0.01).is_empty());
}

#[test]
fn chain_gap_just_inside_tolerance_closes() {
    // closing gap 0.0099 with tol 0.01
    let input = [
        line(0.0, 0.0, 1.0, 0.0),
        line(1.0, 0.0, 1.0, 1.0),
        line(1.0, 1.0, 0.0, 1.0),
        line(0.0, 1.0, 0.0, 0.0099),
    ];
    let out = chain_segments(&input, 0.01);
    assert_eq!(out.len(), 1);
    assert!(out[0].closed);
    assert_eq!(out[0].end(), out[0].start()); // line snapped exactly
}

#[test]
fn chain_gap_just_outside_tolerance_stays_open() {
    // closing gap 0.0101 with tol 0.01
    let input = [
        line(0.0, 0.0, 1.0, 0.0),
        line(1.0, 0.0, 1.0, 1.0),
        line(1.0, 1.0, 0.0, 1.0),
        line(0.0, 1.0, 0.0, 0.0101),
    ];
    let out = chain_segments(&input, 0.01);
    assert_eq!(out.len(), 1);
    assert!(!out[0].closed);
    assert_eq!(out[0].orientation(), None);
    assert_eq!(out[0].signed_area(), 0.0);
}

#[test]
fn chain_middle_gap_just_outside_tolerance_splits() {
    // gap 0.0101 between segment 1 and 2 -> two open chains
    let input = [line(0.0, 0.0, 1.0, 0.0), line(1.0101, 0.0, 2.0, 0.0)];
    let out = chain_segments(&input, 0.01);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|c| !c.closed));
    let out = chain_segments(&input, 0.0102);
    assert_eq!(out.len(), 1);
}

#[test]
fn chain_two_arcs_needing_reversal() {
    // upper semicircle (1,0)->(-1,0) CCW; lower semicircle stored CW (1,0)->(-1,0)
    let upper = arc(0.0, 0.0, 1.0, 0.0, PI);
    let lower_cw = arc(0.0, 0.0, 1.0, TAU, -PI);
    let out = chain_segments(&[upper, lower_cw], 0.01);
    assert_eq!(out.len(), 1);
    let c = &out[0];
    assert!(c.closed && c.is_closed_within(1e-9));
    assert_eq!(c.segments.len(), 2);
    assert!(close(c.signed_area(), PI, T));
    assert_eq!(c.orientation(), Some(Orientation::Ccw));
    assert!(close(c.length(), TAU, T));
}

#[test]
fn chain_full_circle_alone_and_output_order() {
    let full = arc(5.0, 5.0, 2.0, 1.0, TAU);
    let out = chain_segments(
        &[
            line(100.0, 0.0, 101.0, 0.0), // isolated open line first
            full,
            line(0.0, 0.0, 1.0, 0.0),
            line(1.0, 0.0, 1.0, 1.0),
        ],
        0.01,
    );
    assert_eq!(out.len(), 3);
    assert!(!out[0].closed && out[0].segments.len() == 1);
    assert!(out[0].start().approx_eq(p(100.0, 0.0), T));
    assert!(out[1].closed && out[1].segments.len() == 1);
    assert_eq!(out[1].segments[0], full); // arc parameters untouched
    assert!(!out[2].closed && out[2].segments.len() == 2);
}

#[test]
fn chain_backward_extension_with_reversed_candidate() {
    // first segment in the middle; left neighbour stored reversed
    let out = chain_segments(&[line(1.0, 0.0, 2.0, 0.0), line(1.0, 0.0, 0.0, 0.0)], 0.01);
    assert_eq!(out.len(), 1);
    let c = &out[0];
    // contiguous after chaining (either direction is acceptable)
    assert!(c.segments[0].end().approx_eq(c.segments[1].start(), 1e-12));
    assert!(close(c.length(), 2.0, T));
}

// ---------- full-circle contour ----------

#[test]
fn single_full_circle_arc_contour_not_starting_at_zero() {
    let c = Contour {
        segments: vec![arc(3.0, 4.0, 2.0, 1.0, TAU)],
        closed: true,
    };
    assert!(c.is_closed_within(1e-9));
    assert!(close(c.signed_area(), 4.0 * PI, 1e-9));
    assert_eq!(c.orientation(), Some(Orientation::Ccw));
    assert!(close(c.length(), 4.0 * PI, T));
    assert_bbox(c.bbox(), (1.0, 2.0), (5.0, 6.0), T);
    assert!(c.contains_point(p(3.0, 4.0)));
    assert!(c.contains_point(p(4.9, 4.0)));
    assert!(!c.contains_point(p(5.1, 4.0)));
    assert!(c.contains_point(p(3.0, 5.9)));
    assert!(!c.contains_point(p(3.0, 6.1)));
    assert!(!c.contains_point(p(0.9, 4.0)));
    // point on the ray through the arc's start/end vertex (y = center.y + 2 sin 1)
    let y = 4.0 + 2.0 * 1.0f64.sin();
    assert!(c.contains_point(p(3.0, y)));
    assert!(!c.contains_point(p(-3.0, y)));
}

#[test]
fn circle_center_on_ray_through_start_vertex() {
    // circle starts at angle 0, so the +X ray from the center hits the start/end vertex
    let c = circle(0.0, 0.0, 1.0);
    assert!(c.contains_point(p(0.0, 0.0)));
    assert!(c.contains_point(p(-0.5, 0.0)));
    assert!(!c.contains_point(p(-1.5, 0.0)));
    assert!(!c.contains_point(p(1.5, 0.0)));
}

#[test]
fn cw_full_circle_symmetry() {
    let ccw = circle(0.0, 0.0, 3.0);
    let mut cw = ccw.clone();
    cw.reverse();
    assert!(close(cw.signed_area(), -9.0 * PI, 1e-9));
    assert!(close(ccw.signed_area(), 9.0 * PI, 1e-9));
    assert_eq!(cw.orientation(), Some(Orientation::Cw));
    assert!(close(cw.length(), ccw.length(), T));
    assert_bbox(cw.bbox(), (-3.0, -3.0), (3.0, 3.0), T);
    assert!(cw.contains_point(p(0.0, 0.0)));
    assert!(cw.contains_point(p(2.9, 0.1)));
    assert!(!cw.contains_point(p(2.9, 1.0)));
    cw.set_orientation(Orientation::Ccw);
    assert!(close(cw.signed_area(), 9.0 * PI, 1e-9));
}

// ---------- arc bbox ----------

#[test]
fn arc_bbox_ccw_crossing_zero() {
    // -30° .. +30°
    let a = arc(0.0, 0.0, 1.0, -FRAC_PI_6, FRAC_PI_3);
    let c30 = FRAC_PI_6.cos();
    assert_bbox(a.bbox(), (c30, -0.5), (1.0, 0.5), T);
    // same arc expressed with start 330°
    let a = arc(0.0, 0.0, 1.0, 11.0 * FRAC_PI_6, FRAC_PI_3);
    assert_bbox(a.bbox(), (c30, -0.5), (1.0, 0.5), T);
}

#[test]
fn arc_bbox_cw_crossing_zero() {
    // +30° .. -30° clockwise
    let a = arc(0.0, 0.0, 1.0, FRAC_PI_6, -FRAC_PI_3);
    assert_bbox(a.bbox(), (FRAC_PI_6.cos(), -0.5), (1.0, 0.5), T);
}

#[test]
fn arc_bbox_cw_large_sweep() {
    // CW from 135° through 90°, 0°, -90° to -135° (225°)
    let a = arc(0.0, 0.0, 1.0, 3.0 * FRAC_PI_4, -1.5 * PI);
    let c45 = FRAC_PI_4.cos();
    assert_bbox(a.bbox(), (-c45, -1.0), (1.0, 1.0), T);
    // the CCW complement (135° -> 225°) only touches 180°
    let b = arc(0.0, 0.0, 1.0, 3.0 * FRAC_PI_4, FRAC_PI_2);
    assert_bbox(b.bbox(), (-1.0, -c45), (-c45, c45), T);
}

#[test]
fn arc_bbox_start_angle_beyond_two_pi() {
    // start 7π (= π), sweep π/2: (-1,0) -> (0,-1), no interior extreme
    let a = arc(0.0, 0.0, 1.0, 7.0 * PI, FRAC_PI_2);
    assert_bbox(a.bbox(), (-1.0, -1.0), (0.0, 0.0), 1e-9);
    // negative start angle -3π/2 (= π/2), sweep -π: (0,1) -> (0,-1) through (1,0)
    let a = arc(2.0, 2.0, 1.0, -1.5 * PI, -PI);
    assert_bbox(a.bbox(), (2.0, 1.0), (3.0, 3.0), 1e-9);
}

#[test]
fn arc_bbox_extreme_exactly_at_endpoint() {
    // quarter arc 0..90°: extremes are endpoints only
    let a = arc(0.0, 0.0, 2.0, 0.0, FRAC_PI_2);
    assert_bbox(a.bbox(), (0.0, 0.0), (2.0, 2.0), T);
}

// ---------- arc symmetry / bulge ----------

#[test]
fn cw_ccw_arc_length_and_area_symmetry() {
    let ccw = arc(1.0, 1.0, 2.0, 0.4, 2.0);
    let cw = ccw.reversed();
    assert!(close(ccw.length(), 4.0, T));
    assert!(close(cw.length(), 4.0, T));
    // closed chord + arc region in both directions
    let mk = |a: Segment| Contour {
        segments: vec![
            a,
            Segment::Line {
                start: a.end(),
                end: a.start(),
            },
        ],
        closed: true,
    };
    let seg_area = 0.5 * 4.0 * (2.0 - 2.0f64.sin()); // 0.5 r² (θ - sin θ)
    assert!(close(mk(ccw).signed_area(), seg_area, 1e-9));
    assert!(close(mk(cw).signed_area(), -seg_area, 1e-9));
    assert_eq!(mk(ccw).orientation(), Some(Orientation::Ccw));
    assert_eq!(mk(cw).orientation(), Some(Orientation::Cw));
    // bbox is direction independent
    let (b1, b2) = (ccw.bbox(), cw.bbox());
    assert!(b1.min.approx_eq(b2.min, T) && b1.max.approx_eq(b2.max, T));
}

#[test]
fn bulge_zero_and_tiny_give_line() {
    for b in [0.0, 1e-12, -1e-12, -0.0] {
        assert_eq!(
            Segment::arc_from_bulge(p(1.0, 2.0), p(3.0, 4.0), b),
            line(1.0, 2.0, 3.0, 4.0)
        );
    }
    let c = Contour::from_bulge_vertices(&[(p(0.0, 0.0), 0.0), (p(1.0, 0.0), 0.0)], false);
    assert_eq!(c.segments, vec![line(0.0, 0.0, 1.0, 0.0)]);
}

#[test]
fn bulge_negative_one_is_cw_semicircle_mirror_of_positive() {
    let pos = Segment::arc_from_bulge(p(0.0, 0.0), p(2.0, 0.0), 1.0);
    let neg = Segment::arc_from_bulge(p(0.0, 0.0), p(2.0, 0.0), -1.0);
    assert!(pos.end().approx_eq(p(2.0, 0.0), T) && neg.end().approx_eq(p(2.0, 0.0), T));
    // CCW from left to right goes below the chord, CW goes above
    assert_bbox(pos.bbox(), (0.0, -1.0), (2.0, 0.0), T);
    assert_bbox(neg.bbox(), (0.0, 0.0), (2.0, 1.0), T);
    let Segment::Arc { sweep, radius, .. } = neg else {
        panic!("expected arc")
    };
    assert!(close(sweep, -PI, T) && close(radius, 1.0, T));
}

#[test]
fn bulge_polyline_rounded_rectangle_area() {
    // stadium: 4 long, radius-1 round ends
    // vertices (0,0)->(4,0) bulge 1 -> (4,2) -> (0,2) bulge 1 -> back
    let c = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(4.0, 0.0), 1.0),
            (p(4.0, 2.0), 0.0),
            (p(0.0, 2.0), 1.0),
        ],
        true,
    );
    assert!(c.is_closed_within(1e-9));
    assert!(close(c.signed_area(), 8.0 + PI, 1e-9));
    assert!(close(c.length(), 8.0 + TAU, 1e-9));
    assert_bbox(c.bbox(), (-1.0, 0.0), (5.0, 2.0), 1e-9);
    assert!(c.contains_point(p(4.9, 1.0)));
    assert!(c.contains_point(p(-0.9, 1.0)));
    assert!(!c.contains_point(p(4.9, 1.9))); // outside the round end
}

// ---------- transforms ----------

#[test]
fn then_composition_order() {
    let tr = Transform::translate(1.0, 0.0);
    let rot = Transform::rotate(FRAC_PI_2, p(0.0, 0.0));
    assert!(tr.then(&rot).apply(p(0.0, 0.0)).approx_eq(p(0.0, 1.0), T));
    assert!(rot.then(&tr).apply(p(0.0, 0.0)).approx_eq(p(1.0, 0.0), T));
    // then == sequential apply, for a 3-step chain on an arbitrary point
    let sc = Transform::scale(2.5, p(1.0, -1.0));
    let mir = Transform::mirror_x(3.0);
    let q = p(0.3, -7.2);
    let composed = tr.then(&rot).then(&sc).then(&mir);
    let seq = mir.apply(sc.apply(rot.apply(tr.apply(q))));
    assert!(composed.apply(q).approx_eq(seq, 1e-9));
    // associativity
    let alt = tr.then(&rot.then(&sc.then(&mir)));
    assert!(alt.apply(q).approx_eq(seq, 1e-9));
    // identity is neutral
    assert!(Transform::identity()
        .then(&sc)
        .apply(q)
        .approx_eq(sc.apply(q), T));
    assert!(sc
        .then(&Transform::identity())
        .apply(q)
        .approx_eq(sc.apply(q), T));
}

#[test]
fn double_mirror_is_not_mirroring() {
    let m = Transform::mirror_x(1.0).then(&Transform::mirror_y(2.0));
    assert!(!m.is_mirroring()); // = rotation by π about (2,1)
    assert!(m.apply(p(3.0, 1.0)).approx_eq(p(1.0, 1.0), T));
    let c = circle(0.0, 0.0, 1.0).transformed(&m);
    assert_eq!(c.orientation(), Some(Orientation::Ccw));
}

#[test]
fn mirror_contour_flips_orientation() {
    let c = circle(1.0, 2.0, 1.0);
    let m = c.transformed(&Transform::mirror_x(0.0));
    assert_eq!(m.orientation(), Some(Orientation::Cw));
    assert!(close(m.signed_area(), -PI, 1e-9));
    assert_bbox(m.bbox(), (0.0, -3.0), (2.0, -1.0), 1e-9);
    assert!(m.contains_point(p(1.0, -2.0)));
    assert!(m.is_closed_within(1e-9));

    // half-disc with mixed line+arc
    let hd = Contour {
        segments: vec![line(-1.0, 0.0, 1.0, 0.0), arc(0.0, 0.0, 1.0, 0.0, PI)],
        closed: true,
    };
    let m = hd.transformed(&Transform::mirror_y(5.0));
    assert!(m.is_closed_within(1e-9));
    assert_eq!(m.orientation(), Some(Orientation::Cw));
    assert!(close(m.signed_area(), -FRAC_PI_2, 1e-9));
    assert!(m.contains_point(p(10.0, 0.5)));
    assert!(!m.contains_point(p(10.0, -0.5)));
    // each segment endpoint is the mirror of the original endpoint
    for (o, n) in hd.segments.iter().zip(&m.segments) {
        let t = Transform::mirror_y(5.0);
        assert!(n.start().approx_eq(t.apply(o.start()), 1e-9));
        assert!(n.end().approx_eq(t.apply(o.end()), 1e-9));
    }
    // mirroring back restores orientation
    let back = m.transformed(&Transform::mirror_y(5.0));
    assert_eq!(back.orientation(), Some(Orientation::Ccw));
}

#[test]
fn rotate_arc_keeps_endpoints_consistent() {
    let a = arc(1.0, 0.0, 1.0, FRAC_PI_4, 2.5);
    let t = Transform::rotate(1.234, p(-3.0, 2.0));
    let r = a.transformed(&t);
    assert!(r.start().approx_eq(t.apply(a.start()), 1e-9));
    assert!(r.end().approx_eq(t.apply(a.end()), 1e-9));
    assert!(close(r.length(), a.length(), 1e-9));
}

#[test]
fn scale_inch_to_mm_arc() {
    let a = arc(1.0, 1.0, 0.5, 0.0, FRAC_PI_2);
    let s = a.transformed(&Transform::scale(25.4, p(0.0, 0.0)));
    let Segment::Arc {
        center,
        radius,
        sweep,
        ..
    } = s
    else {
        panic!("expected arc")
    };
    assert!(center.approx_eq(p(25.4, 25.4), 1e-9));
    assert!(close(radius, 12.7, 1e-9));
    assert!(close(sweep, FRAC_PI_2, T));
    assert!(s.start().approx_eq(p(38.1, 25.4), 1e-9));
    assert!(s.end().approx_eq(p(25.4, 38.1), 1e-9));
    assert!(close(s.length(), 12.7 * FRAC_PI_2, 1e-9));

    // 1 inch square -> 645.16 mm²
    let sq = polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)])
        .transformed(&Transform::scale(25.4, p(0.0, 0.0)));
    assert!(close(sq.signed_area(), 645.16, 1e-9));
    assert!(close(sq.length(), 101.6, 1e-9));
}

// ---------- point in contour ----------

#[test]
fn contains_point_convex_bulge_far_side() {
    // D shape: square 2x2 whose right side bulges out to x = 3
    let d = Contour {
        segments: vec![
            line(0.0, 0.0, 2.0, 0.0),
            arc(2.0, 1.0, 1.0, -FRAC_PI_2, PI),
            line(2.0, 2.0, 0.0, 2.0),
            line(0.0, 2.0, 0.0, 0.0),
        ],
        closed: true,
    };
    assert!(d.is_closed_within(1e-9));
    assert!(close(d.signed_area(), 4.0 + FRAC_PI_2, 1e-9));
    assert_bbox(d.bbox(), (0.0, 0.0), (3.0, 2.0), 1e-9);
    assert!(d.contains_point(p(2.9, 1.0))); // inside the bulge
    assert!(!d.contains_point(p(3.05, 1.0))); // just beyond the bulge
    assert!(d.contains_point(p(2.5, 1.8))); // dist 0.943 from arc center
    assert!(!d.contains_point(p(2.9, 1.9))); // dist 1.27
    assert!(d.contains_point(p(1.0, 1.0)));
    assert!(!d.contains_point(p(-0.5, 1.0))); // ray crosses left line + arc
}

#[test]
fn contains_point_concave_notch() {
    // same square, right side replaced by an inward CW arc reaching x = 1
    let n = Contour {
        segments: vec![
            line(0.0, 0.0, 2.0, 0.0),
            arc(2.0, 1.0, 1.0, -FRAC_PI_2, -PI),
            line(2.0, 2.0, 0.0, 2.0),
            line(0.0, 2.0, 0.0, 0.0),
        ],
        closed: true,
    };
    assert!(n.is_closed_within(1e-9));
    assert!(close(n.signed_area(), 4.0 - FRAC_PI_2, 1e-9));
    assert_eq!(n.orientation(), Some(Orientation::Ccw));
    assert_bbox(n.bbox(), (0.0, 0.0), (2.0, 2.0), 1e-9);
    assert!(n.contains_point(p(0.5, 1.0)));
    assert!(!n.contains_point(p(1.5, 1.0))); // in the notch
    assert!(!n.contains_point(p(2.5, 1.0)));
    assert!(n.contains_point(p(1.5, 0.1))); // dist 1.03 from arc center
    assert!(!n.contains_point(p(1.9, 1.95))); // dist 0.955 from arc center -> in the notch
}

// ---------- containment tree ----------

#[test]
fn containment_three_levels_plus_siblings_shuffled() {
    let cs = [
        circle(40.0, 50.0, 1.0),    // 0: inside sibling A -> depth 3
        square(50.0, 50.0, 60.0),   // 1: inner -> depth 1
        circle(40.0, 50.0, 5.0),    // 2: sibling A -> depth 2
        square(50.0, 50.0, 100.0),  // 3: outer -> depth 0
        circle(60.0, 50.0, 5.0),    // 4: sibling B -> depth 2
        square(300.0, 300.0, 10.0), // 5: separate -> depth 0
        circle(10.0, 10.0, 5.0),    // 6: in outer, outside inner -> depth 1
    ];
    let t = containment_tree(&cs);
    let got: Vec<_> = t.iter().map(|n| (n.contour, n.parent, n.depth)).collect();
    assert_eq!(
        got,
        vec![
            (0, Some(2), 3),
            (1, Some(3), 1),
            (2, Some(1), 2),
            (3, None, 0),
            (4, Some(1), 2),
            (5, None, 0),
            (6, Some(3), 1),
        ]
    );
}

#[test]
fn containment_ignores_orientation() {
    let mut outer = square(0.0, 0.0, 10.0);
    outer.reverse(); // CW, negative area
    let mut inner = circle(0.0, 0.0, 2.0);
    inner.reverse();
    let t = containment_tree(&[inner, outer]);
    assert_eq!((t[0].parent, t[0].depth), (Some(1), 1));
    assert_eq!((t[1].parent, t[1].depth), (None, 0));
}

#[test]
fn containment_identical_duplicates_not_nested() {
    let t = containment_tree(&[square(0.0, 0.0, 4.0), square(0.0, 0.0, 4.0)]);
    assert!(t.iter().all(|n| n.parent.is_none() && n.depth == 0));
}

#[test]
fn containment_open_contour_never_parent() {
    // open square outline around a closed circle
    let mut open = square(0.0, 0.0, 10.0);
    open.closed = false;
    let t = containment_tree(&[open, circle(0.0, 0.0, 1.0)]);
    assert_eq!((t[0].parent, t[0].depth), (None, 0));
    assert_eq!((t[1].parent, t[1].depth), (None, 0));
    assert!(containment_tree(&[]).is_empty());
}

#[test]
fn containment_bbox_inside_but_point_outside() {
    // L-shaped outer; small square sits in the notch: bbox contained, but not inside
    let l = polygon(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 4.0),
        (4.0, 4.0),
        (4.0, 10.0),
        (0.0, 10.0),
    ]);
    let t = containment_tree(&[l, square(7.0, 7.0, 2.0), square(2.0, 2.0, 1.0)]);
    assert_eq!(t[1].parent, None);
    assert_eq!(t[2].parent, Some(0));
}

// ---------- huge coordinates ----------

#[test]
fn huge_coordinates_1e5_mm() {
    let b = 1e5 + 0.1;
    // relative precision is what matters here: values ~1e10 cancel in the shoelace
    let sq = square(b, b, 10.0);
    assert!(close(sq.signed_area(), 100.0, 1e-5));
    assert_eq!(sq.orientation(), Some(Orientation::Ccw));
    assert!(sq.contains_point(p(b + 4.9, b)));
    assert!(!sq.contains_point(p(b + 5.1, b)));

    let c = circle(b, -b, 5.0);
    assert!(close(c.signed_area(), 25.0 * PI, 1e-5));
    assert_bbox(c.bbox(), (b - 5.0, -b - 5.0), (b + 5.0, -b + 5.0), 1e-9);
    assert!(c.contains_point(p(b + 4.9, -b)));
    assert!(!c.contains_point(p(b + 3.6, -b + 3.6)));

    // tree at huge coordinates
    let t = containment_tree(&[circle(b, b, 1.0), square(b, b, 10.0)]);
    assert_eq!((t[0].parent, t[0].depth), (Some(1), 1));

    // translate back to origin keeps area
    let back = sq.transformed(&Transform::translate(-b, -b));
    assert!(close(back.signed_area(), 100.0, 1e-9));
}

#[test]
fn chained_gaps_area_independent_of_position() {
    // the same 10x10 square with two 0.005 gaps, at the origin and at 1e5 mm:
    // |area| must be ~100 in both places (gaps inside tol must not distort it)
    for b in [0.0, 1e5 + 0.1] {
        let input = [
            line(b, b, b + 10.0, b),
            line(b + 10.005, b, b + 10.0, b + 10.0),
            line(b + 10.0, b + 10.0, b, b + 10.0),
            line(b, b + 10.0, b, b + 0.005),
        ];
        let out = chain_segments(&input, DEFAULT_CHAIN_TOL);
        assert_eq!(out.len(), 1);
        assert!(out[0].closed);
        let a = out[0].signed_area().abs();
        assert!(close(a, 100.0, 0.2), "offset {b}: |area| = {a}");
    }
}
