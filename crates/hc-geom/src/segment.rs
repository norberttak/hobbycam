use crate::{BBox, Contour, Point, Transform, EPS};
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A contour piece: straight line or circular arc.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Segment {
    Line {
        start: Point,
        end: Point,
    },
    /// sweep > 0 = CCW, < 0 = CW; |sweep| in (EPS, 2π]. |sweep| == 2π is a full circle.
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        sweep: f64,
    },
}

/// Point on a circle at `angle`.
pub(crate) fn circle_point(center: Point, radius: f64, angle: f64) -> Point {
    center + Point::new(angle.cos(), angle.sin()) * radius
}

/// Angles from `targets` (taken mod 2π) lying strictly inside the arc sweep,
/// returned in travel order (as actual angles start_angle + signed offset).
pub(crate) fn angles_inside_sweep(start_angle: f64, sweep: f64, targets: &[f64]) -> Vec<f64> {
    let mut offsets: Vec<f64> = targets
        .iter()
        .map(|&t| {
            if sweep > 0.0 {
                (t - start_angle).rem_euclid(TAU)
            } else {
                (start_angle - t).rem_euclid(TAU)
            }
        })
        .filter(|&o| o > 0.0 && o < sweep.abs())
        .collect();
    offsets.sort_by(f64::total_cmp);
    offsets
        .into_iter()
        .map(|o| start_angle + o * sweep.signum())
        .collect()
}

impl Segment {
    pub fn start(&self) -> Point {
        match *self {
            Segment::Line { start, .. } => start,
            Segment::Arc {
                center,
                radius,
                start_angle,
                ..
            } => circle_point(center, radius, start_angle),
        }
    }

    pub fn end(&self) -> Point {
        match *self {
            Segment::Line { end, .. } => end,
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => circle_point(center, radius, start_angle + sweep),
        }
    }

    /// Shortest distance from `p` to the segment.
    pub fn distance_to(&self, p: Point) -> f64 {
        match *self {
            Segment::Line { start, end } => {
                let d = end - start;
                let len2 = d.dot(d);
                if len2 < EPS {
                    return p.distance(start);
                }
                let t = ((p - start).dot(d) / len2).clamp(0.0, 1.0);
                p.distance(start + d * t)
            }
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => {
                let v = p - center;
                let ang = v.y.atan2(v.x);
                let off = if sweep >= 0.0 {
                    (ang - start_angle).rem_euclid(TAU)
                } else {
                    (start_angle - ang).rem_euclid(TAU)
                };
                if off <= sweep.abs() || sweep.abs() >= TAU - EPS {
                    (v.length() - radius).abs()
                } else {
                    p.distance(self.start()).min(p.distance(self.end()))
                }
            }
        }
    }

    /// The point halfway along the segment.
    pub fn midpoint(&self) -> Point {
        match *self {
            Segment::Line { start, end } => (start + end) * 0.5,
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => circle_point(center, radius, start_angle + sweep / 2.0),
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Segment::Line { start, end } => start.distance(end),
            Segment::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        b.include_point(self.start());
        b.include_point(self.end());
        if let Segment::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } = *self
        {
            for a in angles_inside_sweep(start_angle, sweep, &[0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2])
            {
                b.include_point(circle_point(center, radius, a));
            }
        }
        b
    }

    pub fn reversed(&self) -> Segment {
        match *self {
            Segment::Line { start, end } => Segment::Line {
                start: end,
                end: start,
            },
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => Segment::Arc {
                center,
                radius,
                start_angle: start_angle + sweep,
                sweep: -sweep,
            },
        }
    }

    /// Transform with a rigid / mirror / uniform-scale transform.
    pub fn transformed(&self, t: &Transform) -> Segment {
        match *self {
            Segment::Line { start, end } => Segment::Line {
                start: t.apply(start),
                end: t.apply(end),
            },
            Segment::Arc {
                center,
                radius,
                sweep,
                ..
            } => {
                let c = t.apply(center);
                let s = t.apply(self.start()) - c;
                Segment::Arc {
                    center: c,
                    radius: radius * t.determinant().abs().sqrt(),
                    start_angle: s.y.atan2(s.x),
                    sweep: if t.is_mirroring() { -sweep } else { sweep },
                }
            }
        }
    }

    /// DXF bulge (tan of a quarter of the included angle) to segment.
    /// |bulge| < EPS or coincident points give a Line; bulge > 0 is CCW.
    pub fn arc_from_bulge(start: Point, end: Point, bulge: f64) -> Segment {
        if bulge.abs() < EPS || start.distance(end) < EPS {
            return Segment::Line { start, end };
        }
        let chord = end - start;
        let c = chord.length();
        let left = Point::new(-chord.y, chord.x) * (1.0 / c);
        let mid = (start + end) * 0.5;
        // signed distance from chord midpoint to center, along the left normal
        let center = mid + left * (c * (1.0 - bulge * bulge) / (4.0 * bulge));
        let radius = (c * (1.0 + bulge * bulge) / (4.0 * bulge)).abs();
        let s = start - center;
        Segment::Arc {
            center,
            radius,
            start_angle: s.y.atan2(s.x),
            sweep: 4.0 * bulge.atan(),
        }
    }
}

/// Full circle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Point,
    pub radius: f64,
}

impl Circle {
    /// One CCW full-circle arc starting at angle 0, closed.
    pub fn to_contour(&self) -> Contour {
        Contour {
            segments: vec![Segment::Arc {
                center: self.center,
                radius: self.radius,
                start_angle: 0.0,
                sweep: TAU,
            }],
            closed: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_4;

    const T: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn test_08_arc_length_and_endpoints() {
        let a = Segment::Arc {
            center: p(0.0, 0.0),
            radius: 2.0,
            start_angle: 0.0,
            sweep: FRAC_PI_2,
        };
        assert!((a.length() - PI).abs() < T);
        assert!(a.start().approx_eq(p(2.0, 0.0), T));
        assert!(a.end().approx_eq(p(0.0, 2.0), T));
    }

    #[test]
    fn test_09_arc_bbox_crossing_90() {
        let a = Segment::Arc {
            center: p(0.0, 0.0),
            radius: 1.0,
            start_angle: FRAC_PI_4,
            sweep: FRAC_PI_2,
        };
        let b = a.bbox();
        assert!((b.max.y - 1.0).abs() < T);
        assert!((b.min.y - FRAC_PI_4.sin()).abs() < T);
        assert!((b.min.x + FRAC_PI_4.cos()).abs() < T);
        assert!((b.max.x - FRAC_PI_4.cos()).abs() < T);
    }

    #[test]
    fn arc_bbox_wraps_around_zero_and_cw() {
        // CW arc from 45° down through 0° to -45°
        let a = Segment::Arc {
            center: p(0.0, 0.0),
            radius: 1.0,
            start_angle: FRAC_PI_4,
            sweep: -FRAC_PI_2,
        };
        assert!((a.bbox().max.x - 1.0).abs() < T);
        let full = Circle {
            center: p(1.0, 1.0),
            radius: 2.0,
        }
        .to_contour()
        .segments[0];
        let b = full.bbox();
        assert!(b.min.approx_eq(p(-1.0, -1.0), T) && b.max.approx_eq(p(3.0, 3.0), T));
    }

    #[test]
    fn a4_coincident_bulge_points_give_line() {
        let s = Segment::arc_from_bulge(p(1.0, 1.0), p(1.0, 1.0), 0.5);
        assert_eq!(
            s,
            Segment::Line {
                start: p(1.0, 1.0),
                end: p(1.0, 1.0)
            }
        );
    }

    #[test]
    fn test_11b_zero_bulge_is_line() {
        assert_eq!(
            Segment::arc_from_bulge(p(0.0, 0.0), p(2.0, 0.0), 0.0),
            Segment::Line {
                start: p(0.0, 0.0),
                end: p(2.0, 0.0)
            }
        );
    }

    #[test]
    fn bulge_negative_and_quarter() {
        // bulge tan(π/8) = quarter circle CCW from (1,0) to (0,1), center origin
        let a = Segment::arc_from_bulge(p(1.0, 0.0), p(0.0, 1.0), (PI / 8.0).tan());
        let Segment::Arc {
            center,
            radius,
            sweep,
            ..
        } = a
        else {
            panic!("expected arc")
        };
        assert!(center.approx_eq(p(0.0, 0.0), T));
        assert!((radius - 1.0).abs() < T && (sweep - FRAC_PI_2).abs() < T);
        // negative bulge: CW semicircle passes over the top
        let a = Segment::arc_from_bulge(p(-1.0, 0.0), p(1.0, 0.0), -1.0);
        assert!((a.bbox().max.y - 1.0).abs() < T);
        assert!(a.end().approx_eq(p(1.0, 0.0), T));
    }

    #[test]
    fn distance_to_and_midpoint() {
        let l = Segment::Line {
            start: p(0.0, 0.0),
            end: p(10.0, 0.0),
        };
        assert!((l.distance_to(p(5.0, 3.0)) - 3.0).abs() < T);
        assert!((l.distance_to(p(-3.0, 4.0)) - 5.0).abs() < T);
        assert!(l.midpoint().approx_eq(p(5.0, 0.0), T));
        let a = Segment::Arc {
            center: p(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep: FRAC_PI_2,
        };
        assert!((a.distance_to(p(3.0, 3.0)) - (5.0 - 18f64.sqrt())).abs() < T);
        assert!((a.distance_to(p(0.0, -5.0)) - 50f64.sqrt()).abs() < T);
        assert!(a
            .midpoint()
            .approx_eq(p(5.0 * FRAC_PI_4.cos(), 5.0 * FRAC_PI_4.sin()), T));
    }

    #[test]
    fn test_12_rotate_line() {
        let l = Segment::Line {
            start: p(1.0, 0.0),
            end: p(2.0, 0.0),
        }
        .transformed(&Transform::rotate(FRAC_PI_2, p(0.0, 0.0)));
        assert!(l.start().approx_eq(p(0.0, 1.0), T));
        assert!(l.end().approx_eq(p(0.0, 2.0), T));
    }

    #[test]
    fn test_13_mirror_arc() {
        let a = Segment::Arc {
            center: p(1.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            sweep: FRAC_PI_2,
        }
        .transformed(&Transform::mirror_y(0.0));
        let Segment::Arc {
            center,
            sweep,
            radius,
            ..
        } = a
        else {
            panic!("expected arc")
        };
        assert!(center.approx_eq(p(-1.0, 0.0), T));
        assert!((radius - 1.0).abs() < T);
        assert!((sweep + FRAC_PI_2).abs() < T);
        assert!(a.start().approx_eq(p(-2.0, 0.0), T));
        assert!(a.end().approx_eq(p(-1.0, 1.0), T));
    }

    #[test]
    fn test_21a_arc_reversed() {
        let a = Segment::Arc {
            center: p(0.0, 0.0),
            radius: 2.0,
            start_angle: 0.3,
            sweep: 1.2,
        };
        let r = a.reversed();
        assert!(r.start().approx_eq(a.end(), T));
        assert!(r.end().approx_eq(a.start(), T));
        assert!((r.length() - a.length()).abs() < T);
    }
}
