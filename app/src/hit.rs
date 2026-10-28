//! Hit-testing and arc tessellation for the canvas (world mm).

use hc_geom::{BBox, Point, Segment};
use hc_project::{Geom, Item};

const TIE: f64 = 1e-9;
pub const MAX_ARC_PIECES: usize = 512;

/// Points along an arc (n+1 points, exact endpoints) with chord sagitta ≤ `tol`.
pub fn tessellate_arc(center: Point, r: f64, start: f64, sweep: f64, tol: f64) -> Vec<Point> {
    let half = (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    let n = if half > 0.0 {
        (sweep.abs() / (2.0 * half)).ceil()
    } else {
        MAX_ARC_PIECES as f64
    };
    let n = (n as usize).clamp(1, MAX_ARC_PIECES);
    (0..=n)
        .map(|i| {
            let a = start + sweep * i as f64 / n as f64;
            center + Point::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Polyline points of one segment (line: its two endpoints).
pub fn segment_points(seg: &Segment, tol: f64) -> Vec<Point> {
    match *seg {
        Segment::Line { start, end } => vec![start, end],
        Segment::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => tessellate_arc(center, radius, start_angle, sweep, tol),
    }
}

pub fn dist_to_item(p: Point, item: &Item) -> f64 {
    match &item.geom {
        Geom::Point(q) => p.distance(*q),
        Geom::Contour(c) => c.distance_to(p),
    }
}

/// Nearest visible item within `tol`; on a tie the later item wins.
#[cfg(test)]
pub fn pick(items: &[Item], visible: impl Fn(&Item) -> bool, p: Point, tol: f64) -> Option<u64> {
    pick_prefer(items, visible, p, tol, |_| false)
}

/// As `pick`, but on a tie an item with `prefer(id)` (e.g. already selected) beats the later one.
pub fn pick_prefer(
    items: &[Item],
    visible: impl Fn(&Item) -> bool,
    p: Point,
    tol: f64,
    prefer: impl Fn(u64) -> bool,
) -> Option<u64> {
    let mut best: Option<(u64, f64)> = None;
    for it in items.iter().filter(|i| visible(i)) {
        let d = dist_to_item(p, it);
        let better = best.is_none_or(|(bid, bd)| {
            d < bd - TIE || (d <= bd + TIE && (prefer(it.id) || !prefer(bid)))
        });
        if d <= tol && better {
            best = Some((it.id, d));
        }
    }
    best.map(|(id, _)| id)
}

pub fn item_bbox(item: &Item) -> BBox {
    match &item.geom {
        Geom::Point(p) => BBox { min: *p, max: *p },
        Geom::Contour(c) => c.bbox(),
    }
}

fn inside(b: &BBox, p: Point) -> bool {
    p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y
}

/// Liang–Barsky: does segment a-b touch box `b`?
fn line_hits_box(a: Point, e: Point, b: &BBox) -> bool {
    let d = e - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - b.min.x),
        (d.x, b.max.x - a.x),
        (-d.y, a.y - b.min.y),
        (d.y, b.max.y - a.y),
    ] {
        if p.abs() < TIE {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn crosses(item: &Item, b: &BBox, tol: f64) -> bool {
    match &item.geom {
        Geom::Point(p) => inside(b, *p),
        Geom::Contour(c) => c.segments.iter().any(|s| {
            segment_points(s, tol)
                .windows(2)
                .any(|w| line_hits_box(w[0], w[1], b))
        }),
    }
}

/// Window (`crossing = false`): item bbox fully inside. Crossing: inside, or any tessellated piece touches the box.
pub fn box_select(
    items: &[Item],
    visible: impl Fn(&Item) -> bool,
    b: &BBox,
    crossing: bool,
    tol: f64,
) -> Vec<u64> {
    items
        .iter()
        .filter(|i| visible(i))
        .filter(|i| {
            let ib = item_bbox(i);
            let full = !ib.is_empty() && inside(b, ib.min) && inside(b, ib.max);
            full || (crossing && crosses(i, b, tol))
        })
        .map(|i| i.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::bracket_project;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn t5_dist_line_and_arc() {
        let l = Segment::Line {
            start: pt(0.0, 0.0),
            end: pt(10.0, 0.0),
        };
        assert!(close(l.distance_to(pt(5.0, 3.0)), 3.0));
        assert!(close(l.distance_to(pt(-3.0, 4.0)), 5.0));
        let ccw = Segment::Arc {
            center: pt(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep: FRAC_PI_2,
        };
        let cw = Segment::Arc {
            center: pt(0.0, 0.0),
            radius: 5.0,
            start_angle: FRAC_PI_2,
            sweep: -FRAC_PI_2,
        };
        for a in [ccw, cw] {
            assert!(close(a.distance_to(pt(10.0, 0.0)), 5.0));
            assert!(close(a.distance_to(pt(3.0, 3.0)), 5.0 - 18f64.sqrt()));
            assert!(close(a.distance_to(pt(3.0, 3.0)), 0.757359));
            assert!(close(a.distance_to(pt(0.0, -5.0)), 50f64.sqrt()));
        }
    }

    #[test]
    fn t6_tessellate_full_circle() {
        let pts = tessellate_arc(pt(0.0, 0.0), 100.0, 0.0, 2.0 * PI, 0.25);
        assert!(pts.len() - 1 <= MAX_ARC_PIECES);
        assert!(pts[0].distance(pt(100.0, 0.0)) < 1e-9);
        assert!(pts.last().unwrap().distance(pt(100.0, 0.0)) < 1e-9);
        for w in pts.windows(2) {
            let mid = (w[0] + w[1]) * 0.5;
            let sagitta = 100.0 - mid.length();
            assert!(sagitta <= 0.25 + 1e-12, "{sagitta}");
        }
        // tiny radius vs tolerance → one piece, still exact endpoints
        let p = tessellate_arc(pt(1.0, 1.0), 0.1, 0.0, FRAC_PI_2, 5.0);
        assert_eq!(p.len(), 2);
        assert!(p[1].distance(pt(1.0, 1.1)) < 1e-12);
    }

    #[test]
    fn line_box_intersection() {
        let b = BBox {
            min: pt(0.0, 0.0),
            max: pt(10.0, 10.0),
        };
        assert!(line_hits_box(pt(-5.0, 5.0), pt(15.0, 5.0), &b));
        assert!(!line_hits_box(pt(-5.0, 11.0), pt(15.0, 11.0), &b));
        assert!(!line_hits_box(pt(-5.0, 0.0), pt(0.0, -5.0), &b));
        assert!(line_hits_box(pt(-5.0, 5.0), pt(5.0, -5.0), &b)); // touches the corner
        assert!(!line_hits_box(pt(11.0, -5.0), pt(20.0, 5.0), &b));
    }

    #[test]
    fn pick_prefers_selected_on_tie() {
        let c = |id| Item {
            id,
            layer: "0".into(),
            source: hc_project::Source::Drawn,
            geom: Geom::Point(pt(1.0, 1.0)),
        };
        let items = [c(1), c(2)];
        assert_eq!(pick(&items, |_| true, pt(1.0, 1.0), 0.1), Some(2));
        assert_eq!(
            pick_prefer(&items, |_| true, pt(1.0, 1.0), 0.1, |id| id == 1),
            Some(1)
        );
        assert_eq!(
            pick_prefer(&items, |_| true, pt(1.0, 1.0), 0.1, |_| true),
            Some(2)
        );
    }

    #[test]
    fn t7_bracket_pick() {
        let p = bracket_project();
        let tol = 6.0 / 4.0;
        let id = pick(&p.items, |_| true, pt(56.4126, 56.4126), tol).unwrap();
        let it = p.items.iter().find(|i| i.id == id).unwrap();
        match &it.geom {
            Geom::Contour(c) => match c.segments[..] {
                [Segment::Arc { radius, sweep, .. }] => {
                    assert!((radius - 7.9375).abs() < 1e-6);
                    assert!((sweep.abs() - TAU).abs() < 1e-9);
                }
                _ => panic!("not a circle: {c:?}"),
            },
            g => panic!("{g:?}"),
        }
        assert_eq!(pick(&p.items, |_| true, pt(500.0, 500.0), tol), None);
    }

    #[test]
    fn t8_bracket_box_select() {
        let p = bracket_project();
        let b = BBox {
            min: pt(22.86, 22.86),
            max: pt(332.74, 154.94),
        };
        let tol = 0.25 / 4.0;
        let count = |ids: &[u64], layer: &str| {
            ids.iter()
                .filter(|id| p.items.iter().any(|i| i.id == **id && i.layer == layer))
                .count()
        };
        let win = box_select(&p.items, |_| true, &b, false, tol);
        assert_eq!(
            (
                count(&win, "Outline"),
                count(&win, "Hidden"),
                count(&win, "Center")
            ),
            (12, 1, 10)
        );
        assert_eq!(win.len(), 23);
        let cross = box_select(&p.items, |_| true, &b, true, tol);
        assert_eq!(cross.len(), 24);
        assert_eq!(count(&cross, "Title"), 0);
        let no_hidden = box_select(&p.items, |i| i.layer != "Hidden", &b, false, tol);
        assert_eq!(no_hidden.len(), 22);
    }

    #[test]
    fn t9_hidden_not_picked() {
        let p = bracket_project();
        let hidden = p.items.iter().find(|i| i.layer == "Hidden").unwrap();
        let Geom::Contour(c) = &hidden.geom else {
            panic!()
        };
        let Segment::Arc { center, radius, .. } = c.segments[0] else {
            panic!()
        };
        // 45°: away from the centre lines
        let on = center + Point::new(1.0, 1.0) * (radius / 2f64.sqrt());
        assert_eq!(
            pick(&p.items, |i| i.layer == "Hidden", on, 1e-3),
            Some(hidden.id)
        );
        assert_eq!(pick(&p.items, |i| i.layer != "Hidden", on, 1e-3), None);
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    use hc_geom::Contour;
    use hc_project::Source;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn arc(cx: f64, cy: f64, r: f64, start: f64, sweep: f64) -> Segment {
        Segment::Arc {
            center: pt(cx, cy),
            radius: r,
            start_angle: start,
            sweep,
        }
    }

    fn item(id: u64, segs: Vec<Segment>, closed: bool) -> Item {
        Item {
            id,
            layer: "L".into(),
            source: Source::Drawn,
            geom: Geom::Contour(Contour {
                segments: segs,
                closed,
            }),
        }
    }

    fn point_item(id: u64, x: f64, y: f64) -> Item {
        Item {
            id,
            layer: "L".into(),
            source: Source::Drawn,
            geom: Geom::Point(pt(x, y)),
        }
    }

    fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> BBox {
        BBox {
            min: pt(x0, y0),
            max: pt(x1, y1),
        }
    }

    #[test]
    fn zero_length_line_is_a_point() {
        let l = Segment::Line {
            start: pt(2.0, 2.0),
            end: pt(2.0, 2.0),
        };
        assert!((l.distance_to(pt(5.0, 6.0)) - 5.0).abs() < 1e-12);
        assert_eq!(l.distance_to(pt(2.0, 2.0)), 0.0);
        let items = [item(1, vec![l], false)];
        assert_eq!(pick(&items, |_| true, pt(2.5, 2.0), 0.6), Some(1));
        assert_eq!(pick(&items, |_| true, pt(2.5, 2.0), 0.4), None);
        // zero-area box exactly on it: window and crossing both select it
        let b = bx(2.0, 2.0, 2.0, 2.0);
        assert_eq!(box_select(&items, |_| true, &b, false, 0.1), [1]);
        assert_eq!(box_select(&items, |_| true, &b, true, 0.1), [1]);
    }

    #[test]
    fn full_circle_arc_distance_any_angle() {
        for (start, sweep) in [(0.0, TAU), (7.0, TAU), (1.0, -TAU), (-3.0, -TAU)] {
            let a = arc(1.0, 1.0, 5.0, start, sweep);
            for ang in [0.0, 1.0, 2.5, PI, 4.0, 6.0] {
                for d in [0.0, 2.0, 7.0] {
                    let p = pt(1.0 + d * f64::cos(ang), 1.0 + d * f64::sin(ang));
                    let got = a.distance_to(p);
                    assert!(
                        (got - (d - 5.0).abs()).abs() < 1e-9,
                        "{start} {sweep} {ang} {d}: {got}"
                    );
                }
            }
        }
    }

    #[test]
    fn cw_and_ccw_arc_same_distance_everywhere() {
        // same geometric arc, both directions, also with start angles outside [0, 2π)
        let pairs = [
            (arc(0.0, 0.0, 3.0, 1.0, 2.0), arc(0.0, 0.0, 3.0, 3.0, -2.0)),
            (
                arc(0.0, 0.0, 3.0, -0.5, 1.0),
                arc(0.0, 0.0, 3.0, 0.5 + TAU, -1.0),
            ),
            (
                arc(2.0, -1.0, 1.0, 5.0, 4.0),
                arc(2.0, -1.0, 1.0, 9.0, -4.0),
            ),
        ];
        for (a, b) in pairs {
            for i in 0..36 {
                let t = i as f64 * TAU / 36.0;
                for r in [0.0, 0.5, 2.9, 3.0, 4.0, 10.0] {
                    let p = pt(r * t.cos(), r * t.sin());
                    let (da, db) = (a.distance_to(p), b.distance_to(p));
                    assert!((da - db).abs() < 1e-9, "{p:?}: {da} vs {db}");
                }
            }
        }
    }

    #[test]
    fn arc_distance_outside_sweep_uses_nearest_endpoint() {
        // quarter arc 0..90°, query at 180°: nearest endpoint is (0,5)
        let a = arc(0.0, 0.0, 5.0, 0.0, FRAC_PI_2);
        assert!((a.distance_to(pt(-5.0, 0.0)) - 50f64.sqrt()).abs() < 1e-12);
        // at the centre every arc point is r away, but outside the sweep only endpoints count: still 5
        assert!((a.distance_to(pt(0.0, 0.0)) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn pick_exactly_at_threshold_and_tie() {
        let items = [point_item(1, 0.0, 0.0)];
        assert_eq!(pick(&items, |_| true, pt(3.0, 4.0), 5.0), Some(1));
        assert_eq!(pick(&items, |_| true, pt(3.0, 4.0), 4.999_999), None);
        assert_eq!(pick(&items, |_| true, pt(0.0, 0.0), 0.0), Some(1));
        // identical geometry: the item drawn last wins
        let l = Segment::Line {
            start: pt(0.0, 0.0),
            end: pt(10.0, 0.0),
        };
        let items = [
            item(7, vec![l], false),
            item(3, vec![l], false),
            item(5, vec![l], false),
        ];
        assert_eq!(pick(&items, |_| true, pt(4.0, 1.0), 2.0), Some(5));
        assert_eq!(pick(&items, |i| i.id != 5, pt(4.0, 1.0), 2.0), Some(3));
        // a strictly nearer earlier item still beats a later one
        let far = Segment::Line {
            start: pt(0.0, 1.5),
            end: pt(10.0, 1.5),
        };
        let items = [item(1, vec![l], false), item(2, vec![far], false)];
        assert_eq!(pick(&items, |_| true, pt(4.0, 0.5), 2.0), Some(1));
        assert_eq!(pick(&[], |_| true, pt(0.0, 0.0), 1.0), None);
    }

    #[test]
    fn zero_area_box() {
        let line = item(
            1,
            vec![Segment::Line {
                start: pt(0.0, 0.0),
                end: pt(10.0, 0.0),
            }],
            false,
        );
        let items = [line, point_item(2, 5.0, 0.0), point_item(3, 6.0, 0.0)];
        let b = bx(5.0, 0.0, 5.0, 0.0);
        // window: only what fits in a point
        assert_eq!(box_select(&items, |_| true, &b, false, 0.1), [2]);
        // crossing: the line passes through it as well
        assert_eq!(box_select(&items, |_| true, &b, true, 0.1), [1, 2]);
        // zero-width (vertical) box crossing the line
        let v = bx(3.0, -1.0, 3.0, 1.0);
        assert_eq!(box_select(&items, |_| true, &v, true, 0.1), [1]);
        assert!(box_select(&items, |_| true, &v, false, 0.1).is_empty());
        // zero-area box in empty space
        assert!(box_select(&items, |_| true, &bx(5.0, 3.0, 5.0, 3.0), true, 0.1).is_empty());
    }

    #[test]
    fn box_inside_large_circle_selects_nothing() {
        let c = item(1, vec![arc(0.0, 0.0, 100.0, 0.0, TAU)], true);
        let items = [c];
        let b = bx(-10.0, -10.0, 10.0, 10.0);
        assert!(box_select(&items, |_| true, &b, true, 0.01).is_empty());
        assert!(box_select(&items, |_| true, &b, false, 0.01).is_empty());
        // just touching the circle edge does select in crossing mode
        let edge = bx(95.0, -1.0, 105.0, 1.0);
        assert_eq!(box_select(&items, |_| true, &edge, true, 0.01), [1]);
        // a box around the whole circle selects it in window mode
        let all = bx(-100.0, -100.0, 100.0, 100.0);
        assert_eq!(box_select(&items, |_| true, &all, false, 0.01), [1]);
    }

    #[test]
    fn box_crossing_arc_bulge_but_not_chord() {
        // open upper semicircle r10: bulge at (0,10), chord along y=0 (not part of the item)
        let items = [item(1, vec![arc(0.0, 0.0, 10.0, 0.0, PI)], false)];
        let bulge = bx(-0.5, 9.0, 0.5, 11.0);
        assert_eq!(box_select(&items, |_| true, &bulge, true, 0.01), [1]);
        // inside the arc's bbox and on its chord, but touching no part of the curve
        let chord = bx(-1.0, -1.0, 1.0, 1.0);
        assert!(box_select(&items, |_| true, &chord, true, 0.01).is_empty());
        // the CW version of the same arc behaves the same
        let items = [item(1, vec![arc(0.0, 0.0, 10.0, PI, -PI)], false)];
        assert_eq!(box_select(&items, |_| true, &bulge, true, 0.01), [1]);
        assert!(box_select(&items, |_| true, &chord, true, 0.01).is_empty());
    }

    fn max_sagitta(c: Point, r: f64, pts: &[Point]) -> f64 {
        pts.windows(2)
            .map(|w| r - ((w[0] + w[1]) * 0.5).distance(c))
            .fold(0.0, f64::max)
    }

    #[test]
    fn tessellate_error_bound_and_endpoints() {
        for r in [0.3, 1.0, 7.9375, 100.0, 1000.0] {
            for (start, sweep) in [(0.0, TAU), (1.0, 0.3), (2.0, -2.5), (-7.0, PI)] {
                for tol in [0.25, 0.01, 0.001] {
                    let c = pt(3.0, -4.0);
                    let pts = tessellate_arc(c, r, start, sweep, tol);
                    let n = pts.len() - 1;
                    assert!((1..=MAX_ARC_PIECES).contains(&n));
                    let s = c + Point::new(start.cos(), start.sin()) * r;
                    let e = c + Point::new((start + sweep).cos(), (start + sweep).sin()) * r;
                    assert!(pts[0].distance(s) < 1e-9 && pts[n].distance(e) < 1e-9);
                    if n < MAX_ARC_PIECES {
                        let sag = max_sagitta(c, r, &pts);
                        assert!(sag <= tol + 1e-9, "r {r} sweep {sweep} tol {tol}: {sag}");
                    }
                    // all points on the circle
                    assert!(pts.iter().all(|p| (p.distance(c) - r).abs() < 1e-9));
                }
            }
        }
    }

    #[test]
    fn tessellate_tiny_zero_and_huge_radius() {
        // tiny: one chord, exact endpoints, no NaN
        for r in [1e-9, 1e-3, 0.1] {
            let p = tessellate_arc(pt(1.0, 1.0), r, 0.0, FRAC_PI_2, 0.25);
            assert_eq!(p.len(), 2, "r {r}");
            assert!(p.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
        }
        // r = 0 and tol = 0 must not produce NaN or an empty list
        let p = tessellate_arc(pt(1.0, 1.0), 0.0, 0.0, TAU, 0.25);
        assert!(p.len() >= 2 && p.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
        let p = tessellate_arc(pt(0.0, 0.0), 5.0, 0.0, PI, 0.0);
        assert_eq!(p.len() - 1, MAX_ARC_PIECES);
        // huge radius: capped at 512 pieces (spec); the sagitta then exceeds tol by design
        let r = 1e6;
        let p = tessellate_arc(pt(0.0, 0.0), r, 0.0, TAU, 0.25);
        assert_eq!(p.len() - 1, MAX_ARC_PIECES);
        assert!(p[0].distance(pt(r, 0.0)) < 1e-6);
        assert!(p[MAX_ARC_PIECES].distance(pt(r, 0.0)) < 1e-6);
        let expected = r * (1.0 - (PI / 512.0).cos());
        assert!((max_sagitta(pt(0.0, 0.0), r, &p) - expected).abs() < 1e-3);
        // zero sweep: one degenerate chord
        let p = tessellate_arc(pt(0.0, 0.0), 5.0, 1.0, 0.0, 0.25);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn point_items_in_boxes_and_hidden_filter() {
        let items = [point_item(1, 1.0, 1.0), point_item(2, 50.0, 50.0)];
        let b = bx(0.0, 0.0, 10.0, 10.0);
        assert_eq!(box_select(&items, |_| true, &b, false, 0.1), [1]);
        assert_eq!(box_select(&items, |_| true, &b, true, 0.1), [1]);
        assert!(box_select(&items, |i| i.id != 1, &b, true, 0.1).is_empty());
        assert_eq!(pick(&items, |i| i.id != 1, pt(1.0, 1.0), 1.0), None);
    }
}
