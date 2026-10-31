//! Trim and extend contour segments at limiting segments (M22).

use crate::{BBox, Contour, Point, Segment, EPS};
use std::f64::consts::TAU;

/// Cut points closer than this (mm, along the segment) to each other or to a segment end are merged / ignored.
pub const TRIM_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trimmed {
    /// What remains, 1 or 2 open contours; parts[0] is the one that keeps the item id.
    pub parts: Vec<Contour>,
    /// The removed piece (one segment), for the preview.
    pub removed: Segment,
}

/// (segment index, distance along that segment, distance from p) of the segment nearest to `p`; first wins ties.
/// None for an empty contour.
pub fn nearest_segment(c: &Contour, p: Point) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, s) in c.segments.iter().enumerate() {
        let d = s.closest(p);
        let dist = p.distance(s.point_at(d));
        if best.is_none_or(|b| dist < b.2) {
            best = Some((i, d, dist));
        }
    }
    best
}

fn is_full(s: &Segment) -> bool {
    matches!(*s, Segment::Arc { sweep, .. } if sweep.abs() >= TAU - EPS)
}

/// Distances along `seg` (from its start) where it meets `limits`, ascending, merged within TRIM_TOL. Partial segment:
/// only d in (TRIM_TOL, len - TRIM_TOL). Full circle: d in [0, len), the wrap handled as described in M22.
pub fn cuts(seg: &Segment, limits: &[Segment]) -> Vec<f64> {
    let len = seg.length();
    let full = is_full(seg);
    let mut ds: Vec<f64> = limits
        .iter()
        .flat_map(|l| seg.intersections(l))
        .map(|q| seg.closest(q))
        .filter_map(|d| {
            if full {
                Some(if d >= len - TRIM_TOL { 0.0 } else { d })
            } else {
                (d > TRIM_TOL && d < len - TRIM_TOL).then_some(d)
            }
        })
        .collect();
    ds.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::with_capacity(ds.len());
    for d in ds {
        if out.last().is_none_or(|&l| d - l > TRIM_TOL) {
            out.push(d);
        }
    }
    if full && out.len() > 1 && out[0] + len - out[out.len() - 1] <= TRIM_TOL {
        out.pop();
    }
    out
}

/// The part of `s` between distances `from` < `to`; arcs may use from < 0 or to > len for the circle wrap.
fn sub(s: &Segment, from: f64, to: f64) -> Segment {
    match *s {
        Segment::Line { start, end } => Segment::Line {
            start: if from <= 0.0 { start } else { s.point_at(from) },
            end: if to >= s.length() {
                end
            } else {
                s.point_at(to)
            },
        },
        Segment::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => {
            let sg = sweep.signum();
            Segment::Arc {
                center,
                radius,
                start_angle: start_angle + sg * from / radius,
                sweep: sg * (to - from) / radius,
            }
        }
    }
}

fn open(segments: Vec<Segment>) -> Contour {
    Contour {
        segments,
        closed: false,
    }
}

/// Removes the piece of segment `seg` around distance `d`. None = nothing to trim.
pub fn trim(c: &Contour, seg: usize, d: f64, limits: &[Segment]) -> Option<Trimmed> {
    let s = c.segments.get(seg)?;
    let len = s.length();
    let cs = cuts(s, limits);
    if c.segments.len() == 1 && is_full(s) {
        if cs.len() < 2 {
            return None;
        }
        let hi = cs.iter().copied().find(|&x| x > d).unwrap_or(cs[0] + len);
        let lo = cs
            .iter()
            .copied()
            .rfind(|&x| x <= d)
            .unwrap_or(cs[cs.len() - 1] - len);
        return Some(Trimmed {
            parts: vec![open(vec![sub(s, hi, lo + len)])],
            removed: sub(s, lo, hi),
        });
    }
    if cs.is_empty() {
        return None;
    }
    let lo = cs.iter().copied().rfind(|&x| x < d).unwrap_or(0.0);
    let hi = cs.iter().copied().find(|&x| x >= d).unwrap_or(len);
    let removed = sub(s, lo, hi);
    let mut a: Vec<Segment> = c.segments[..seg].to_vec();
    if lo > 0.0 {
        a.push(sub(s, 0.0, lo));
    }
    let mut b: Vec<Segment> = Vec::new();
    if hi < len {
        b.push(sub(s, hi, len));
    }
    b.extend_from_slice(&c.segments[seg + 1..]);
    let parts = if c.closed {
        b.extend(a);
        vec![open(b)]
    } else {
        [a, b]
            .into_iter()
            .filter(|v| !v.is_empty())
            .map(open)
            .collect()
    };
    Some(Trimmed { parts, removed })
}

/// Lengthens the start or end segment of an open contour to the nearest limit.
/// Some((new contour, added piece)); None when closed, empty, no limits or no hit.
pub fn extend(c: &Contour, end: End, limits: &[Segment]) -> Option<(Contour, Segment)> {
    if c.closed || c.segments.is_empty() || limits.is_empty() {
        return None;
    }
    let o = match end {
        End::End => *c.segments.last()?,
        End::Start => c.segments[0].reversed(),
    };
    let q = match o {
        Segment::Line { start: a, end: b } => {
            let len = a.distance(b);
            if len < EPS {
                return None;
            }
            let u = (b - a) * (1.0 / len);
            let mut bb = BBox::empty();
            bb.include_point(b);
            for l in limits {
                bb = bb.union(&l.bbox());
            }
            let big = 1.0 + bb.min.distance(bb.max);
            Segment::Line {
                start: b,
                end: b + u * big,
            }
        }
        Segment::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => {
            let rest = TAU - sweep.abs();
            if rest < TRIM_TOL / radius {
                return None;
            }
            Segment::Arc {
                center,
                radius,
                start_angle: start_angle + sweep,
                sweep: sweep.signum() * rest,
            }
        }
    };
    let hit = limits
        .iter()
        .flat_map(|l| q.intersections(l))
        .map(|p| q.closest(p))
        .filter(|&h| h > TRIM_TOL)
        .min_by(f64::total_cmp)?;
    let added = sub(&q, 0.0, hit);
    let mut segments = c.segments.clone();
    let idx = if end == End::End {
        segments.len() - 1
    } else {
        0
    };
    segments[idx] = match (segments[idx], end) {
        (Segment::Line { start, .. }, End::End) => Segment::Line {
            start,
            end: added.end(),
        },
        (Segment::Line { end: e, .. }, End::Start) => Segment::Line {
            start: added.end(),
            end: e,
        },
        (
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            },
            _,
        ) => {
            let (sg, k) = (sweep.signum(), hit / radius);
            Segment::Arc {
                center,
                radius,
                start_angle: if end == End::End {
                    start_angle
                } else {
                    start_angle - sg * k
                },
                sweep: sweep + sg * k,
            }
        }
    };
    Some((open(segments), added))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shapes, Circle};
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, PI};

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }
    fn l(a: (f64, f64), b: (f64, f64)) -> Segment {
        Segment::Line {
            start: p(a.0, a.1),
            end: p(b.0, b.1),
        }
    }
    fn poly(pts: &[(f64, f64)]) -> Contour {
        open(pts.windows(2).map(|w| l(w[0], w[1])).collect())
    }
    fn same(a: &Segment, b: &Segment) {
        assert_eq!(
            std::mem::discriminant(a),
            std::mem::discriminant(b),
            "{a:?} {b:?}"
        );
        for (x, y) in [
            (a.start(), b.start()),
            (a.end(), b.end()),
            (a.midpoint(), b.midpoint()),
        ] {
            assert!(x.distance(y) < 1e-9, "{a:?} != {b:?}");
        }
    }
    fn same_c(c: &Contour, want: &[Segment]) {
        assert_eq!(c.segments.len(), want.len(), "{c:?}");
        assert!(!c.closed);
        for (a, b) in c.segments.iter().zip(want) {
            same(a, b);
        }
    }
    fn arc(c: (f64, f64), r: f64, a: f64, sw: f64) -> Segment {
        Segment::Arc {
            center: p(c.0, c.1),
            radius: r,
            start_angle: a,
            sweep: sw,
        }
    }

    #[test]
    fn t1_end_piece() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [l((4.0, -5.0), (4.0, 5.0))];
        let t = trim(&c, 0, 8.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 1);
        same_c(&t.parts[0], &[l((0.0, 0.0), (4.0, 0.0))]);
        same(&t.removed, &l((4.0, 0.0), (10.0, 0.0)));
        let t = trim(&c, 0, 1.0, &lim).unwrap();
        same_c(&t.parts[0], &[l((4.0, 0.0), (10.0, 0.0))]);
    }

    #[test]
    fn t2_middle_split() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [l((3.0, -5.0), (3.0, 5.0)), l((7.0, -5.0), (7.0, 5.0))];
        let t = trim(&c, 0, 5.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 2);
        same_c(&t.parts[0], &[l((0.0, 0.0), (3.0, 0.0))]);
        same_c(&t.parts[1], &[l((7.0, 0.0), (10.0, 0.0))]);
        same(&t.removed, &l((3.0, 0.0), (7.0, 0.0)));
    }

    #[test]
    fn t3_line_circle() {
        let c = poly(&[(-10.0, 0.0), (10.0, 0.0)]);
        let lim = [Circle {
            center: p(0.0, 0.0),
            radius: 5.0,
        }
        .to_contour()
        .segments[0]];
        let t = trim(&c, 0, 10.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 2);
        same_c(&t.parts[0], &[l((-10.0, 0.0), (-5.0, 0.0))]);
        same_c(&t.parts[1], &[l((5.0, 0.0), (10.0, 0.0))]);
        same(&t.removed, &l((-5.0, 0.0), (5.0, 0.0)));
    }

    #[test]
    fn t4_circle_to_arc() {
        let c = Circle {
            center: p(0.0, 0.0),
            radius: 5.0,
        }
        .to_contour();
        let lim = [l((0.0, -10.0), (0.0, 10.0))];
        let cs = cuts(&c.segments[0], &lim);
        assert_eq!(cs.len(), 2);
        assert!((cs[0] - 2.5 * PI).abs() < 1e-9 && (cs[1] - 7.5 * PI).abs() < 1e-9);
        let t = trim(&c, 0, 0.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 1);
        let a = t.parts[0].segments[0];
        assert!(!t.parts[0].closed);
        same(&a, &arc((0.0, 0.0), 5.0, FRAC_PI_2, PI));
        assert!(a.start().distance(p(0.0, 5.0)) < 1e-9);
        assert!(a.end().distance(p(0.0, -5.0)) < 1e-9);
        assert!(a.midpoint().distance(p(-5.0, 0.0)) < 1e-9);
        assert!(t.removed.start().distance(p(0.0, -5.0)) < 1e-9);
        assert!(t.removed.end().distance(p(0.0, 5.0)) < 1e-9);
        assert!(t.removed.midpoint().distance(p(5.0, 0.0)) < 1e-9);
        assert!(trim(&c, 0, 0.0, &[l((-10.0, 5.0), (10.0, 5.0))]).is_none());
    }

    #[test]
    fn t5_arc_line() {
        let c = open(vec![arc((0.0, 0.0), 10.0, 0.0, PI)]);
        let t = trim(&c, 0, 5.0 * PI, &[l((5.0, -20.0), (5.0, 20.0))]).unwrap();
        assert_eq!(t.parts.len(), 1);
        same_c(&t.parts[0], &[arc((0.0, 0.0), 10.0, 0.0, FRAC_PI_3)]);
        assert!(t.parts[0].end().distance(p(5.0, 8.660254037844386)) < 1e-9);
        same(
            &t.removed,
            &arc((0.0, 0.0), 10.0, FRAC_PI_3, 2.0 * FRAC_PI_3),
        );
        assert!(t.removed.end().distance(p(-10.0, 0.0)) < 1e-9);
    }

    #[test]
    fn t6_closed_rect_opened() {
        let c = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 0.0).unwrap();
        let lim = [l((10.0, -5.0), (10.0, 15.0))];
        let t = trim(&c, 2, 10.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 1);
        same_c(
            &t.parts[0],
            &[
                l((10.0, 10.0), (0.0, 10.0)),
                l((0.0, 10.0), (0.0, 0.0)),
                l((0.0, 0.0), (30.0, 0.0)),
                l((30.0, 0.0), (30.0, 10.0)),
            ],
        );
        assert!((t.parts[0].length() - 60.0).abs() < 1e-9);
        same(&t.removed, &l((30.0, 10.0), (10.0, 10.0)));
        let t = trim(&c, 0, 20.0, &lim).unwrap();
        same_c(
            &t.parts[0],
            &[
                l((30.0, 0.0), (30.0, 10.0)),
                l((30.0, 10.0), (0.0, 10.0)),
                l((0.0, 10.0), (0.0, 0.0)),
                l((0.0, 0.0), (10.0, 0.0)),
            ],
        );
    }

    #[test]
    fn t7_polyline_split() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0)]);
        let lim = [l((-5.0, 3.0), (25.0, 3.0)), l((-5.0, 7.0), (25.0, 7.0))];
        let t = trim(&c, 1, 5.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 2);
        same_c(
            &t.parts[0],
            &[l((0.0, 0.0), (10.0, 0.0)), l((10.0, 0.0), (10.0, 3.0))],
        );
        same_c(
            &t.parts[1],
            &[l((10.0, 7.0), (10.0, 10.0)), l((10.0, 10.0), (20.0, 10.0))],
        );
    }

    #[test]
    fn t8_nothing() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        for x in [20.0, 10.0] {
            let lim = [l((x, -5.0), (x, 5.0))];
            assert!(cuts(&c.segments[0], &lim).is_empty());
            assert!(trim(&c, 0, 5.0, &lim).is_none());
        }
    }

    #[test]
    fn t9_self_limit() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, -5.0)]);
        let lim = [c.segments[1], c.segments[2]];
        let t = trim(&c, 0, 9.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 2);
        same_c(&t.parts[0], &[l((0.0, 0.0), (6.666666666666667, 0.0))]);
        same_c(
            &t.parts[1],
            &[l((10.0, 0.0), (10.0, 10.0)), l((10.0, 10.0), (5.0, -5.0))],
        );
    }

    #[test]
    fn t10_nearest_segment() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0)]);
        let (i, d, dist) = nearest_segment(&c, p(10.2, 5.0)).unwrap();
        assert_eq!(i, 1);
        assert!((d - 5.0).abs() < 1e-9 && (dist - 0.2).abs() < 1e-9);
        assert!(nearest_segment(&open(vec![]), p(0.0, 0.0)).is_none());
    }

    fn ext(c: &Contour, e: End, lim: &[Segment]) -> Option<Contour> {
        extend(c, e, lim).map(|r| r.0)
    }

    #[test]
    fn e1_line_to_circle() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [Circle {
            center: p(20.0, 0.0),
            radius: 5.0,
        }
        .to_contour()
        .segments[0]];
        let (n, added) = extend(&c, End::End, &lim).unwrap();
        same_c(&n, &[l((0.0, 0.0), (15.0, 0.0))]);
        same(&added, &l((10.0, 0.0), (15.0, 0.0)));
    }

    #[test]
    fn e2_start_to_line() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [l((-5.0, -5.0), (-5.0, 5.0))];
        same_c(
            &ext(&c, End::Start, &lim).unwrap(),
            &[l((-5.0, 0.0), (10.0, 0.0))],
        );
        assert!(extend(&c, End::End, &lim).is_none());
    }

    #[test]
    fn e3_arc_to_line() {
        let c = open(vec![arc((0.0, 0.0), 10.0, 0.0, FRAC_PI_2)]);
        let (n, added) = extend(&c, End::End, &[l((-20.0, 5.0), (0.0, 5.0))]).unwrap();
        same_c(&n, &[arc((0.0, 0.0), 10.0, 0.0, 5.0 * PI / 6.0)]);
        assert!(n.end().distance(p(-8.660254037844386, 5.0)) < 1e-9);
        same(&added, &arc((0.0, 0.0), 10.0, FRAC_PI_2, FRAC_PI_3));
        let n = ext(&c, End::End, &[l((5.0, -20.0), (5.0, -5.0))]).unwrap();
        same_c(&n, &[arc((0.0, 0.0), 10.0, 0.0, 5.0 * PI / 3.0)]);
        assert!(n.end().distance(p(5.0, -8.660254037844386)) < 1e-9);
        assert!(extend(&c, End::End, &[l((20.0, -5.0), (20.0, 5.0))]).is_none());
    }

    #[test]
    fn e4_cw_arc_start() {
        let c = open(vec![arc((0.0, 0.0), 10.0, FRAC_PI_2, -FRAC_PI_2)]);
        let n = ext(&c, End::Start, &[l((-20.0, 5.0), (0.0, 5.0))]).unwrap();
        same_c(
            &n,
            &[arc((0.0, 0.0), 10.0, 5.0 * PI / 6.0, -5.0 * PI / 6.0)],
        );
        assert!(n.start().distance(p(-8.660254037844386, 5.0)) < 1e-9);
        assert!(n.end().distance(p(10.0, 0.0)) < 1e-9);
    }

    #[test]
    fn e5_next_limit() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [l((10.0, -5.0), (10.0, 5.0)), l((20.0, -5.0), (20.0, 5.0))];
        same_c(
            &ext(&c, End::End, &lim).unwrap(),
            &[l((0.0, 0.0), (20.0, 0.0))],
        );
    }

    #[test]
    fn e6_polyline() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let lim = [l((-5.0, 20.0), (25.0, 20.0))];
        same_c(
            &ext(&c, End::End, &lim).unwrap(),
            &[l((0.0, 0.0), (10.0, 0.0)), l((10.0, 0.0), (10.0, 20.0))],
        );
        assert!(extend(&c, End::Start, &lim).is_none());
    }

    #[test]
    fn e7_none() {
        let r = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 0.0).unwrap();
        let lim = [l((5.0, -5.0), (5.0, 5.0))];
        assert!(extend(&r, End::End, &lim).is_none());
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        assert!(extend(&c, End::End, &[]).is_none());
        assert!(extend(&c, End::End, &[l((5.0, 5.0), (5.0, 10.0))]).is_none());
    }

    fn valid(c: &Contour) {
        for s in &c.segments {
            assert!(s.length() > 0.0, "zero-length segment in {c:?}");
        }
        for w in c.segments.windows(2) {
            assert!(w[0].end().distance(w[1].start()) < 1e-9, "gap in {c:?}");
        }
    }

    #[test]
    fn x1_circle_three_crossings_and_one() {
        let circ = Circle {
            center: p(0.0, 0.0),
            radius: 5.0,
        }
        .to_contour();
        // three limits through the circle: 2 vertical lines -> 4 cuts
        let lim = [l((-3.0, -10.0), (-3.0, 10.0)), l((3.0, -10.0), (3.0, 10.0))];
        let cs = cuts(&circ.segments[0], &lim);
        assert_eq!(cs.len(), 4);
        let t = trim(&circ, 0, 0.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 1);
        valid(&t.parts[0]);
        let total = t.parts[0].length() + t.removed.length();
        assert!((total - 10.0 * PI).abs() < 1e-9);
        // a single crossing (limit ends on the circle) -> refused
        assert!(trim(&circ, 0, 0.0, &[l((5.0, 0.0), (8.0, 0.0))]).is_none());
    }

    #[test]
    fn x2_cw_arc_trim_and_cut_at_arc_end() {
        let a = Contour {
            segments: vec![arc((0.0, 0.0), 10.0, FRAC_PI_2, -FRAC_PI_2)],
            closed: false,
        };
        // vertical line x=5 crosses the CW arc once at (5, 8.66)
        let lim = [l((5.0, -20.0), (5.0, 20.0))];
        let t = trim(&a, 0, 1.0, &lim).unwrap();
        assert_eq!(t.parts.len(), 1);
        valid(&t.parts[0]);
        assert!(t.parts[0].start().distance(p(5.0, 75f64.sqrt())) < 1e-9);
        assert!(t.parts[0].end().distance(p(10.0, 0.0)) < 1e-9);
        // limit touching only the arc end point: not a cut
        assert!(trim(&a, 0, 1.0, &[l((10.0, -5.0), (10.0, 5.0))]).is_none());
    }

    #[test]
    fn x3_tangent_line_to_circle_is_one_cut() {
        let circ = Circle {
            center: p(0.0, 0.0),
            radius: 5.0,
        }
        .to_contour();
        let lim = [l((-10.0, 5.0), (10.0, 5.0))];
        assert_eq!(cuts(&circ.segments[0], &lim).len(), 1);
        // tangent line as the *clicked* segment: touch point at the middle trims it in two
        let c = poly(&[(-10.0, 5.0), (10.0, 5.0)]);
        let t = trim(&c, 0, 2.0, &[circ.segments[0]]).unwrap();
        assert_eq!(t.parts.len(), 1);
        valid(&t.parts[0]);
    }

    #[test]
    fn x4_vertex_touch_and_tolerance_edges() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        // cut 0.5e-6 inside the end: ignored; 2e-6 inside: counts
        assert!(trim(
            &c,
            0,
            5.0,
            &[l((10.0 - 0.5e-6, -1.0), (10.0 - 0.5e-6, 1.0))]
        )
        .is_none());
        let t = trim(
            &c,
            0,
            9.999999,
            &[l((10.0 - 2e-6, -1.0), (10.0 - 2e-6, 1.0))],
        )
        .unwrap();
        assert_eq!(t.parts.len(), 1);
        valid(&t.parts[0]);
        // two limits within 1e-6 of each other merge into one cut
        let lim = [
            l((4.0, -1.0), (4.0, 1.0)),
            l((4.0 + 5e-7, -1.0), (4.0 + 5e-7, 1.0)),
        ];
        assert_eq!(cuts(&c.segments[0], &lim).len(), 1);
    }

    #[test]
    fn x5_nearly_parallel_and_tiny_segments() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        // nearly parallel limit crossing at x=5
        let lim = [l((0.0, -1e-7), (10.0, 1e-7))];
        if let Some(t) = trim(&c, 0, 2.0, &lim) {
            for part in &t.parts {
                valid(part);
            }
        }
        // tiny limit segment crossing the line
        let lim = [l((4.0, -1e-5), (4.0, 1e-5))];
        let t = trim(&c, 0, 8.0, &lim).unwrap();
        valid(&t.parts[0]);
        // tiny clicked segment inside a polyline must not produce zero-length output
        let c = poly(&[(0.0, 0.0), (5.0, 0.0), (5.0, 1e-4), (10.0, 1e-4)]);
        let lim = [l((2.0, -1.0), (2.0, 1.0))];
        let t = trim(&c, 0, 4.0, &lim).unwrap();
        for part in &t.parts {
            valid(part);
        }
    }

    #[test]
    fn x6_extend_circle_near_and_far() {
        let c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let circ = Circle {
            center: p(20.0, 0.0),
            radius: 5.0,
        }
        .to_contour();
        let (n, _) = extend(&c, End::End, &[circ.segments[0]]).unwrap();
        same(&n.segments[0], &l((0.0, 0.0), (15.0, 0.0)));
        // starting inside the circle: first hit is the far side
        let c = poly(&[(0.0, 0.0), (20.0, 0.0)]);
        let (n, _) = extend(&c, End::End, &[circ.segments[0]]).unwrap();
        same(&n.segments[0], &l((0.0, 0.0), (25.0, 0.0)));
        valid(&n);
    }

    #[test]
    fn x7_extend_arc_bounded_by_full_circle() {
        let a = Contour {
            segments: vec![arc((0.0, 0.0), 10.0, 0.0, FRAC_PI_2)],
            closed: false,
        };
        // limit just before the arc start, reached after nearly a full turn
        let lim = [l((10.0, -1.0), (11.0, -1.0))];
        // does not intersect the circle at all -> None
        assert!(extend(&a, End::End, &lim).is_none());
        let lim = [l((9.0, -1.0), (11.0, -1.0))];
        let (n, _) = extend(&a, End::End, &lim).unwrap();
        match n.segments[0] {
            Segment::Arc { sweep, .. } => assert!(sweep > FRAC_PI_2 && sweep < 2.0 * PI),
            _ => panic!(),
        }
        valid(&n);
        // a nearly-full circle: no room left
        let full = Contour {
            segments: vec![arc((0.0, 0.0), 10.0, 0.0, 2.0 * PI - 1e-9)],
            closed: false,
        };
        assert!(extend(&full, End::End, &[l((0.0, -20.0), (0.0, 20.0))]).is_none());
    }

    #[test]
    fn x8_repeated_extend_goes_to_next_limit() {
        let mut c = poly(&[(0.0, 0.0), (10.0, 0.0)]);
        let lim = [
            l((10.0, -5.0), (10.0, 5.0)),
            l((20.0, -5.0), (20.0, 5.0)),
            l((30.0, -5.0), (30.0, 5.0)),
        ];
        for x in [20.0, 30.0] {
            c = extend(&c, End::End, &lim).unwrap().0;
            assert!((c.end().x - x).abs() < 1e-9);
            assert_eq!(c.segments.len(), 1);
        }
        assert!(extend(&c, End::End, &lim).is_none());
    }

    #[test]
    fn x9_closed_rect_trim_orientation_and_endpoints() {
        let r = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 0.0).unwrap();
        let lim = [l((10.0, -5.0), (10.0, 15.0)), l((20.0, -5.0), (20.0, 15.0))];
        // click between the two cuts on the top edge
        let t = trim(&r, 2, 15.0, &lim).unwrap();
        let c = &t.parts[0];
        valid(c);
        assert!(!c.closed);
        assert!(c.start().distance(p(10.0, 10.0)) < 1e-9);
        assert!(c.end().distance(p(20.0, 10.0)) < 1e-9);
        assert!((c.length() - 70.0).abs() < 1e-9);
    }
}
