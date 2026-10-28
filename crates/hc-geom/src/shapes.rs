//! Drawing-tool shapes as plain contours (M10).

use crate::{Circle, Contour, Point, Segment, EPS};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Open, one Line. None if a or b is non-finite or |b − a| < EPS.
pub fn line(a: Point, b: Point) -> Option<Contour> {
    if !finite(a) || !finite(b) || a.distance(b) < EPS {
        return None;
    }
    Some(Contour {
        segments: vec![Segment::Line { start: a, end: b }],
        closed: false,
    })
}

/// Closed, CCW, start (min.x + r, min.y). None if non-finite, w or h < EPS, r < 0, or r > min(w, h)/2 + EPS (r is then clamped to
/// min/2). Edges shorter than EPS are omitted (r = h/2 gives a slot, w = h = 2r gives four arcs).
pub fn rect(min: Point, w: f64, h: f64, r: f64) -> Option<Contour> {
    if !finite(min) || !w.is_finite() || !h.is_finite() || !r.is_finite() {
        return None;
    }
    let half = w.min(h) / 2.0;
    if w < EPS || h < EPS || r < 0.0 || r > half + EPS {
        return None;
    }
    let r = r.min(half);
    let (x0, y0, x1, y1) = (min.x, min.y, min.x + w, min.y + h);
    // (edge start, edge end, corner arc centre, arc start angle), CCW from the bottom edge
    let sides = [
        (
            Point::new(x0 + r, y0),
            Point::new(x1 - r, y0),
            Point::new(x1 - r, y0 + r),
            -FRAC_PI_2,
        ),
        (
            Point::new(x1, y0 + r),
            Point::new(x1, y1 - r),
            Point::new(x1 - r, y1 - r),
            0.0,
        ),
        (
            Point::new(x1 - r, y1),
            Point::new(x0 + r, y1),
            Point::new(x0 + r, y1 - r),
            FRAC_PI_2,
        ),
        (
            Point::new(x0, y1 - r),
            Point::new(x0, y0 + r),
            Point::new(x0 + r, y0 + r),
            PI,
        ),
    ];
    let mut segments = Vec::new();
    for (start, end, center, start_angle) in sides {
        if start.distance(end) >= EPS {
            segments.push(Segment::Line { start, end });
        }
        if r >= EPS {
            segments.push(Segment::Arc {
                center,
                radius: r,
                start_angle,
                sweep: FRAC_PI_2,
            });
        }
    }
    Some(Contour {
        segments,
        closed: true,
    })
}

/// `Circle { center, radius }.to_contour()`. None if non-finite or r < EPS.
pub fn circle(center: Point, r: f64) -> Option<Contour> {
    if !finite(center) || !r.is_finite() || r < EPS {
        return None;
    }
    Some(Circle { center, radius: r }.to_contour())
}

/// Tolerance (mm) for recognising a drawn-tool shape in arbitrary (e.g. imported) geometry.
pub const RECOGNIZE_TOL: f64 = 1e-3;
const RECOGNIZE_ANG: f64 = 1e-6;

/// A contour that is exactly one of the drawing-tool shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    /// `a` = segment start, `b` = segment end.
    Line {
        a: Point,
        b: Point,
    },
    /// `min` = bbox min corner; `r` = 0 for sharp corners.
    Rect {
        min: Point,
        w: f64,
        h: f64,
        r: f64,
    },
    Circle {
        center: Point,
        r: f64,
    },
}

/// Recognises a line, circle or axis-aligned (optionally rounded) rectangle; None for anything else.
pub fn recognize(c: &Contour) -> Option<Primitive> {
    let segs = &c.segments;
    if segs.is_empty() {
        return None;
    }
    if !c.closed {
        return match segs[..] {
            [Segment::Line { start, end }] => Some(Primitive::Line { a: start, b: end }),
            _ => None,
        };
    }
    recognize_circle(segs)
        .map(|(center, r)| Primitive::Circle { center, r })
        .or_else(|| recognize_rect(c))
}

fn recognize_circle(segs: &[Segment]) -> Option<(Point, f64)> {
    let Segment::Arc {
        center: c0,
        radius: r0,
        sweep: s0,
        ..
    } = segs[0]
    else {
        return None;
    };
    let mut total = 0.0;
    for s in segs {
        let Segment::Arc {
            center,
            radius,
            sweep,
            ..
        } = *s
        else {
            return None;
        };
        if center.distance(c0) > RECOGNIZE_TOL
            || (radius - r0).abs() > RECOGNIZE_TOL
            || sweep.signum() != s0.signum()
        {
            return None;
        }
        total += sweep.abs();
    }
    ((total - TAU).abs() <= RECOGNIZE_ANG * segs.len() as f64).then_some((c0, r0))
}

fn recognize_rect(c: &Contour) -> Option<Primitive> {
    let segs = &c.segments;
    let n = segs.len();
    if n != 4 && n != 8 {
        return None;
    }
    // true = horizontal line, false = vertical line
    let mut dirs = Vec::new();
    for s in segs {
        if let Segment::Line { start, end } = *s {
            let (h, v) = (
                (end.y - start.y).abs() <= RECOGNIZE_TOL,
                (end.x - start.x).abs() <= RECOGNIZE_TOL,
            );
            if h == v {
                return None;
            }
            dirs.push(h);
        }
    }
    if dirs.len() != 4 || dirs.windows(2).any(|w| w[0] == w[1]) || dirs[0] == dirs[dirs.len() - 1] {
        return None;
    }
    let mut r = 0.0;
    if n == 8 {
        let area_sign = c.signed_area().signum();
        let mut r0 = None;
        for i in 0..8 {
            let arc = matches!(segs[i], Segment::Arc { .. });
            if arc != matches!(segs[(i + 1) % 8], Segment::Line { .. }) {
                return None;
            }
            if let Segment::Arc { radius, sweep, .. } = segs[i] {
                let r0 = *r0.get_or_insert(radius);
                if (sweep.abs() - FRAC_PI_2).abs() > RECOGNIZE_ANG
                    || sweep.signum() != area_sign
                    || (radius - r0).abs() > RECOGNIZE_TOL
                {
                    return None;
                }
            }
        }
        r = r0?;
    }
    let b = c.bbox();
    Some(Primitive::Rect {
        min: b.min,
        w: b.width(),
        h: b.height(),
        r,
    })
}

/// `new` with the orientation of `old` (when both are closed and non-degenerate) and, for a closed `new`, its
/// segments rotated so it starts at the segment start nearest to `old`'s start mapped proportionally from `old`'s bbox
/// into `new`'s bbox. Keeps manual tab fractions in place when a rectangle is rebuilt.
pub fn align_like(old: &Contour, mut new: Contour) -> Contour {
    if let Some(o) = old.orientation() {
        new.set_orientation(o);
    }
    if !new.closed || new.segments.is_empty() || old.segments.is_empty() {
        return new;
    }
    let (ob, nb) = (old.bbox(), new.bbox());
    let rel = |v: f64, lo: f64, size: f64| if size > EPS { (v - lo) / size } else { 0.0 };
    let s = old.start();
    let t = Point::new(
        nb.min.x + rel(s.x, ob.min.x, ob.width()) * nb.width(),
        nb.min.y + rel(s.y, ob.min.y, ob.height()) * nb.height(),
    );
    // first index on ties
    let k = (0..new.segments.len())
        .min_by(|&i, &j| {
            new.segments[i]
                .start()
                .distance(t)
                .total_cmp(&new.segments[j].start().distance(t))
        })
        .unwrap();
    new.segments.rotate_left(k);
    new
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Orientation, Transform};

    const T: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn connected(c: &Contour) -> bool {
        let n = c.segments.len();
        (0..n).all(|i| {
            c.segments[i]
                .end()
                .approx_eq(c.segments[(i + 1) % n].start(), T)
        })
    }

    #[test]
    fn t01_line() {
        let c = line(p(0.0, 0.0), p(3.0, 4.0)).unwrap();
        assert!(!c.closed);
        assert_eq!(c.segments.len(), 1);
        assert!((c.length() - 5.0).abs() < T);
        assert!(line(p(1.0, 1.0), p(1.0, 1.0)).is_none());
        assert!(line(p(f64::NAN, 0.0), p(1.0, 1.0)).is_none());
        assert!(line(p(0.0, 0.0), p(1.0, f64::INFINITY)).is_none());
    }

    #[test]
    fn t02_rect_sharp() {
        let c = rect(p(10.0, 20.0), 30.0, 10.0, 0.0).unwrap();
        assert!(c.closed);
        assert_eq!(c.segments.len(), 4);
        assert!(c.segments.iter().all(|s| matches!(s, Segment::Line { .. })));
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
        assert!(c.start().approx_eq(p(10.0, 20.0), T));
        let b = c.bbox();
        assert!(b.min.approx_eq(p(10.0, 20.0), T) && b.max.approx_eq(p(40.0, 30.0), T));
        assert!((c.signed_area() - 300.0).abs() < T);
        assert!((c.length() - 80.0).abs() < T);
        assert!(connected(&c));
    }

    #[test]
    fn t02_rect_rounded() {
        let c = rect(p(10.0, 20.0), 30.0, 10.0, 2.0).unwrap();
        assert_eq!(c.segments.len(), 8);
        for (i, s) in c.segments.iter().enumerate() {
            match s {
                Segment::Line { .. } => assert_eq!(i % 2, 0),
                Segment::Arc { sweep, .. } => {
                    assert_eq!(i % 2, 1);
                    assert!((sweep - FRAC_PI_2).abs() < T);
                }
            }
        }
        assert!(c.start().approx_eq(p(12.0, 20.0), T));
        let b = c.bbox();
        assert!(b.min.approx_eq(p(10.0, 20.0), T) && b.max.approx_eq(p(40.0, 30.0), T));
        assert!((c.length() - (64.0 + 4.0 * PI)).abs() < T);
        assert!((c.signed_area() - (300.0 - (4.0 - PI) * 4.0)).abs() < T);
        assert!(connected(&c));

        let slot = rect(p(10.0, 20.0), 30.0, 10.0, 5.0).unwrap();
        assert_eq!(slot.segments.len(), 6);
        assert!(connected(&slot));
        let round = rect(p(0.0, 0.0), 10.0, 10.0, 5.0).unwrap();
        assert_eq!(round.segments.len(), 4);
        assert!(round
            .segments
            .iter()
            .all(|s| matches!(s, Segment::Arc { .. })));
        assert!((round.signed_area() - 25.0 * PI).abs() < T);
        assert!(connected(&round));
    }

    #[test]
    fn t02_rect_invalid() {
        let m = p(10.0, 20.0);
        assert!(rect(m, 30.0, 10.0, 5.0001).is_none());
        assert!(rect(m, 30.0, 10.0, -1.0).is_none());
        assert!(rect(m, 0.0, 10.0, 0.0).is_none());
        assert!(rect(m, f64::NAN, 10.0, 0.0).is_none());
        assert!(rect(p(f64::NAN, 0.0), 1.0, 1.0, 0.0).is_none());
        assert!(rect(m, 10.0, 10.0, f64::NAN).is_none());
        // within EPS above min/2 is clamped
        let c = rect(m, 10.0, 10.0, 5.0 + 1e-10).unwrap();
        assert_eq!(c.segments.len(), 4);
    }

    #[test]
    fn t03_circle() {
        let c = circle(p(1.0, 2.0), 2.5).unwrap();
        let k = c.as_circle().unwrap();
        assert_eq!(k.radius, 2.5);
        assert_eq!(k.center, p(1.0, 2.0));
        assert!(circle(p(1.0, 2.0), 0.0).is_none());
        assert!(circle(p(1.0, 2.0), f64::NAN).is_none());
    }

    fn cw_rect() -> Contour {
        let v: Vec<(Point, f64)> = [(40.0, 30.0), (40.0, 20.0), (10.0, 20.0), (10.0, 30.0)]
            .iter()
            .map(|&(x, y)| (p(x, y), 0.0))
            .collect();
        Contour::from_bulge_vertices(&v, true)
    }

    #[test]
    fn t04_recognize_line() {
        let c = line(p(0.0, 0.0), p(3.0, 4.0)).unwrap();
        assert_eq!(
            recognize(&c),
            Some(Primitive::Line {
                a: p(0.0, 0.0),
                b: p(3.0, 4.0)
            })
        );
        let pl = Contour::from_bulge_vertices(
            &[(p(0.0, 0.0), 0.0), (p(1.0, 0.0), 0.0), (p(1.0, 1.0), 0.0)],
            false,
        );
        assert_eq!(recognize(&pl), None);
    }

    #[test]
    fn t05_recognize_circle() {
        let c = circle(p(1.0, 2.0), 2.5).unwrap();
        assert_eq!(
            recognize(&c),
            Some(Primitive::Circle {
                center: p(1.0, 2.0),
                r: 2.5
            })
        );
        let mut two =
            Contour::from_bulge_vertices(&[(p(0.0, 0.0), -1.0), (p(10.0, 0.0), -1.0)], true);
        match recognize(&two) {
            Some(Primitive::Circle { center, r }) => {
                assert!(center.approx_eq(p(5.0, 0.0), T) && (r - 5.0).abs() < T)
            }
            o => panic!("{o:?}"),
        }
        if let Segment::Arc { radius, .. } = &mut two.segments[1] {
            *radius += 0.01;
        }
        assert_eq!(recognize(&two), None);
        let half = Contour::from_bulge_vertices(&[(p(0.0, 0.0), 1.0), (p(10.0, 0.0), 0.0)], true);
        assert_eq!(recognize(&half), None);
    }

    fn is_rect(c: &Contour, w: f64, r: f64) -> bool {
        match recognize(c) {
            Some(Primitive::Rect {
                min,
                w: w2,
                h,
                r: r2,
            }) => {
                min.approx_eq(p(10.0, 20.0), 1e-3)
                    && (w2 - w).abs() < 1e-3
                    && (h - 10.0).abs() < T
                    && (r2 - r).abs() < T
            }
            _ => false,
        }
    }

    #[test]
    fn t06_recognize_rect() {
        assert!(is_rect(
            &rect(p(10.0, 20.0), 30.0, 10.0, 0.0).unwrap(),
            30.0,
            0.0
        ));
        assert!(is_rect(
            &rect(p(10.0, 20.0), 30.0, 10.0, 2.0).unwrap(),
            30.0,
            2.0
        ));
        assert!(is_rect(&cw_rect(), 30.0, 0.0));
        let mut off = cw_rect();
        let set = |c: &mut Contour, x: f64| {
            // move vertex (40,30) = start of seg 0 / end of seg 3
            if let Segment::Line { start, .. } = &mut c.segments[0] {
                start.x = x;
            }
            if let Segment::Line { end, .. } = &mut c.segments[3] {
                end.x = x;
            }
        };
        set(&mut off, 40.0005);
        assert!(is_rect(&off, 30.0, 0.0));
        set(&mut off, 40.002);
        assert_eq!(recognize(&off), None);
        assert_eq!(
            recognize(&rect(p(10.0, 20.0), 30.0, 10.0, 5.0).unwrap()),
            None
        );
        let rot = rect(p(0.0, 0.0), 20.0, 10.0, 0.0)
            .unwrap()
            .transformed(&Transform::rotate(0.1, p(0.0, 0.0)));
        assert_eq!(recognize(&rot), None);
        let l = Contour::from_bulge_vertices(
            &[
                (p(0.0, 0.0), 0.0),
                (p(10.0, 0.0), 0.0),
                (p(10.0, 5.0), 0.0),
                (p(5.0, 5.0), 0.0),
                (p(5.0, 10.0), 0.0),
                (p(0.0, 10.0), 0.0),
            ],
            true,
        );
        assert_eq!(recognize(&l), None);
        let tri = Contour::from_bulge_vertices(
            &[(p(0.0, 0.0), 0.0), (p(10.0, 0.0), 0.0), (p(5.0, 5.0), 0.0)],
            true,
        );
        assert_eq!(recognize(&tri), None);
        let mut bad = rect(p(10.0, 20.0), 30.0, 10.0, 2.0).unwrap();
        if let Segment::Arc { radius, .. } = &mut bad.segments[5] {
            *radius = 2.5;
        }
        assert_eq!(recognize(&bad), None);
        assert_eq!(
            recognize(&rect(p(0.0, 0.0), 10.0, 10.0, 5.0).unwrap()),
            Some(Primitive::Circle {
                center: p(5.0, 5.0),
                r: 5.0
            })
        );
    }

    #[test]
    fn t07_align_like() {
        let old = cw_rect();
        let a = align_like(&old, rect(p(0.0, 0.0), 60.0, 20.0, 0.0).unwrap());
        assert_eq!(a.orientation(), Some(Orientation::Cw));
        assert!(a.start().approx_eq(p(60.0, 20.0), T));
        assert_eq!(a.segments.len(), 4);
        let b = a.bbox();
        assert!(b.min.approx_eq(p(0.0, 0.0), T) && b.max.approx_eq(p(60.0, 20.0), T));
        let a = align_like(&old, rect(p(0.0, 0.0), 60.0, 20.0, 2.0).unwrap());
        assert_eq!(a.orientation(), Some(Orientation::Cw));
        assert_eq!(a.segments.len(), 8);
        assert!(a.start().approx_eq(p(60.0, 18.0), T) || a.start().approx_eq(p(58.0, 20.0), T));
        let l = line(p(0.0, 0.0), p(1.0, 1.0)).unwrap();
        assert_eq!(align_like(&old, l.clone()), l);
    }
}
