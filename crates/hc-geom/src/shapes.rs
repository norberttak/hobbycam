//! Drawing-tool shapes as plain contours (M10).

use crate::{Circle, Contour, Point, Segment, EPS};
use std::f64::consts::{FRAC_PI_2, PI};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Orientation;

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
}
