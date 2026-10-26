use crate::segment::angles_inside_sweep;
use crate::{BBox, Point, Segment, Transform, EPS};
use serde::{Deserialize, Serialize};
use std::f64::consts::FRAC_PI_2;

/// Connected sequence of segments, each starting where the previous ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    pub segments: Vec<Segment>,
    pub closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Ccw,
    Cw,
}

impl Contour {
    /// Build from (vertex, bulge to next vertex) pairs, e.g. a DXF LWPOLYLINE.
    /// When closed, the last vertex connects back to the first using its bulge.
    /// Consecutive duplicate vertices (closer than EPS) are skipped; the later one's bulge is kept.
    pub fn from_bulge_vertices(v: &[(Point, f64)], closed: bool) -> Contour {
        let mut uniq: Vec<(Point, f64)> = Vec::with_capacity(v.len());
        for &(pt, bulge) in v {
            match uniq.last_mut() {
                Some(last) if last.0.distance(pt) < EPS => last.1 = bulge,
                _ => uniq.push((pt, bulge)),
            }
        }
        if closed && uniq.len() > 1 && uniq[uniq.len() - 1].0.distance(uniq[0].0) < EPS {
            uniq.pop();
        }
        let v = &uniq;
        let n = v.len();
        let count = if closed { n } else { n.saturating_sub(1) };
        let segments = (0..count)
            .map(|i| Segment::arc_from_bulge(v[i].0, v[(i + 1) % n].0, v[i].1))
            .collect();
        Contour { segments, closed }
    }

    /// Panics on an empty contour.
    pub fn start(&self) -> Point {
        self.segments[0].start()
    }

    /// Panics on an empty contour.
    pub fn end(&self) -> Point {
        self.segments[self.segments.len() - 1].end()
    }

    pub fn length(&self) -> f64 {
        self.segments.iter().map(Segment::length).sum()
    }

    pub fn bbox(&self) -> BBox {
        self.segments
            .iter()
            .fold(BBox::empty(), |b, s| b.union(&s.bbox()))
    }

    pub fn is_closed_within(&self, tol: f64) -> bool {
        !self.segments.is_empty() && self.end().approx_eq(self.start(), tol)
    }

    /// Start point of every segment; segment i ends at vertices[(i+1) % n] in a closed contour,
    /// so all joints (including the closing one) use one identical point.
    fn vertices(&self) -> Vec<Point> {
        self.segments.iter().map(Segment::start).collect()
    }

    /// Positive for CCW. Open or empty contours give 0.0.
    pub fn signed_area(&self) -> f64 {
        if !self.closed || self.segments.is_empty() {
            return 0.0;
        }
        let v = self.vertices();
        let n = v.len();
        // shoelace relative to the first vertex keeps precision far from the origin
        let o = v[0];
        self.segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let chord = 0.5 * (v[i] - o).cross(v[(i + 1) % n] - o);
                match *s {
                    Segment::Line { .. } => chord,
                    Segment::Arc { radius, sweep, .. } => {
                        chord + 0.5 * radius * radius * (sweep - sweep.sin())
                    }
                }
            })
            .sum()
    }

    pub fn orientation(&self) -> Option<Orientation> {
        let a = self.signed_area();
        if !self.closed || a.abs() < EPS {
            None
        } else if a > 0.0 {
            Some(Orientation::Ccw)
        } else {
            Some(Orientation::Cw)
        }
    }

    pub fn reverse(&mut self) {
        self.segments.reverse();
        for s in &mut self.segments {
            *s = s.reversed();
        }
    }

    /// No-op for open or degenerate contours.
    pub fn set_orientation(&mut self, o: Orientation) {
        if self.orientation().is_some_and(|cur| cur != o) {
            self.reverse();
        }
    }

    /// Even-odd ray cast towards +X. Open contours contain nothing; on-boundary result is unspecified.
    pub fn contains_point(&self, p: Point) -> bool {
        if !self.closed || self.segments.is_empty() {
            return false;
        }
        let v = self.vertices();
        let n = v.len();
        let mut crossings = 0;
        for (i, s) in self.segments.iter().enumerate() {
            let (s_pt, e_pt) = (v[i], v[(i + 1) % n]);
            match *s {
                Segment::Line { .. } => {
                    let (start, end) = (s_pt, e_pt);
                    if (start.y > p.y) != (end.y > p.y) {
                        let x = start.x + (p.y - start.y) * (end.x - start.x) / (end.y - start.y);
                        if x > p.x {
                            crossings += 1;
                        }
                    }
                }
                Segment::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep,
                } => {
                    // split into y-monotone pieces at the top/bottom of the circle
                    let mut angles = vec![start_angle];
                    angles.extend(angles_inside_sweep(
                        start_angle,
                        sweep,
                        &[FRAC_PI_2, 3.0 * FRAC_PI_2],
                    ));
                    angles.push(start_angle + sweep);
                    // y of each split point: exact endpoints, exact top/bottom in between
                    let last = angles.len() - 1;
                    let ys: Vec<f64> = angles
                        .iter()
                        .enumerate()
                        .map(|(k, &a)| match k {
                            0 => s_pt.y,
                            k if k == last => e_pt.y,
                            _ => center.y + radius * a.sin().signum(),
                        })
                        .collect();
                    for (k, w) in angles.windows(2).enumerate() {
                        let (y0, y1) = (ys[k], ys[k + 1]);
                        if (y0 > p.y) != (y1 > p.y) {
                            let dy = p.y - center.y;
                            let dx = (radius * radius - dy * dy).max(0.0).sqrt();
                            let right_half = (0.5 * (w[0] + w[1])).cos() >= 0.0;
                            let x = if right_half {
                                center.x + dx
                            } else {
                                center.x - dx
                            };
                            if x > p.x {
                                crossings += 1;
                            }
                        }
                    }
                }
            }
        }
        crossings % 2 == 1
    }

    pub fn transformed(&self, t: &Transform) -> Contour {
        Contour {
            segments: self.segments.iter().map(|s| s.transformed(t)).collect(),
            closed: self.closed,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Circle;
    use std::f64::consts::PI;

    const T: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// Closed polygon of lines through the given points.
    pub(crate) fn polygon(pts: &[(f64, f64)]) -> Contour {
        let v: Vec<(Point, f64)> = pts.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
        Contour::from_bulge_vertices(&v, true)
    }

    pub(crate) fn unit_square() -> Contour {
        polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)])
    }

    fn half_disc() -> Contour {
        Contour {
            segments: vec![
                Segment::Line {
                    start: p(-1.0, 0.0),
                    end: p(1.0, 0.0),
                },
                Segment::Arc {
                    center: p(0.0, 0.0),
                    radius: 1.0,
                    start_angle: 0.0,
                    sweep: PI,
                },
            ],
            closed: true,
        }
    }

    #[test]
    fn test_01_unit_square_ccw() {
        let c = unit_square();
        assert!(c.closed && c.is_closed_within(T));
        assert!((c.signed_area() - 1.0).abs() < T);
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
        assert!((c.length() - 4.0).abs() < T);
    }

    #[test]
    fn test_02_reversed_square_cw() {
        let mut c = polygon(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]);
        assert!((c.signed_area() + 1.0).abs() < T);
        assert_eq!(c.orientation(), Some(Orientation::Cw));
        c.set_orientation(Orientation::Ccw);
        assert!((c.signed_area() - 1.0).abs() < T);
        assert!(c.is_closed_within(T));
    }

    #[test]
    fn test_07_circle_contour() {
        let c = Circle {
            center: p(0.0, 0.0),
            radius: 1.0,
        }
        .to_contour();
        assert!(c.closed && c.is_closed_within(T));
        assert!((c.signed_area() - PI).abs() < T);
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
        assert!((c.length() - 2.0 * PI).abs() < T);
    }

    #[test]
    fn test_10_half_disc() {
        let c = half_disc();
        assert!(c.is_closed_within(T));
        assert!((c.signed_area() - PI / 2.0).abs() < T);
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
    }

    #[test]
    fn test_11_lower_half_disc_from_bulge() {
        let arc = Segment::arc_from_bulge(p(-1.0, 0.0), p(1.0, 0.0), 1.0);
        let Segment::Arc {
            center,
            radius,
            sweep,
            ..
        } = arc
        else {
            panic!("expected arc")
        };
        assert!(center.approx_eq(p(0.0, 0.0), T));
        assert!((radius - 1.0).abs() < T);
        assert!((sweep - PI).abs() < T);
        assert!((arc.bbox().min.y + 1.0).abs() < T); // passes (0,-1)
        let c = Contour {
            segments: vec![
                Segment::Line {
                    start: p(1.0, 0.0),
                    end: p(-1.0, 0.0),
                },
                arc,
            ],
            closed: true,
        };
        assert!(c.is_closed_within(T));
        assert!((c.signed_area() - PI / 2.0).abs() < T);
        assert_eq!(c.orientation(), Some(Orientation::Ccw));
        // same contour via from_bulge_vertices
        let v = Contour::from_bulge_vertices(&[(p(1.0, 0.0), 0.0), (p(-1.0, 0.0), 1.0)], true);
        assert!((v.signed_area() - PI / 2.0).abs() < T);
    }

    #[test]
    fn test_14_mirrored_square_cw() {
        let c = unit_square().transformed(&Transform::mirror_y(0.0));
        assert_eq!(c.orientation(), Some(Orientation::Cw));
        assert!((c.signed_area().abs() - 1.0).abs() < T);
    }

    #[test]
    fn test_15_scale_and_translate() {
        let c = Circle {
            center: p(0.0, 0.0),
            radius: 1.0,
        }
        .to_contour()
        .transformed(&Transform::scale(2.0, p(0.0, 0.0)));
        assert!((c.signed_area() - 4.0 * PI).abs() < T);
        let b = unit_square()
            .transformed(&Transform::translate(5.0, -3.0))
            .bbox();
        assert!(b.min.approx_eq(p(5.0, -3.0), T));
        assert!(b.max.approx_eq(p(6.0, -2.0), T));
    }

    #[test]
    fn test_17_contains_point() {
        let sq = unit_square();
        assert!(sq.contains_point(p(0.5, 0.5)));
        assert!(!sq.contains_point(p(1.5, 0.5)));
        let circle = Circle {
            center: p(0.0, 0.0),
            radius: 1.0,
        }
        .to_contour();
        assert!(circle.contains_point(p(0.9, 0.0)));
        assert!(!circle.contains_point(p(0.0, 1.1)));
        assert!(circle.contains_point(p(0.7, 0.7)));
        let hd = half_disc();
        assert!(hd.contains_point(p(0.0, 0.5)));
        assert!(!hd.contains_point(p(0.0, -0.5)));
    }

    #[test]
    fn contains_point_cw_arcs_and_open() {
        let mut circle = Circle {
            center: p(3.0, -2.0),
            radius: 1.0,
        }
        .to_contour();
        circle.reverse();
        assert!(circle.contains_point(p(2.2, -2.3)));
        assert!(!circle.contains_point(p(1.9, -2.0)));
        let open = Contour {
            closed: false,
            ..unit_square()
        };
        assert!(!open.contains_point(p(0.5, 0.5)));
        assert_eq!(open.signed_area(), 0.0);
        assert_eq!(open.orientation(), None);
    }

    #[test]
    fn a4_from_bulge_vertices_skips_duplicates() {
        let c = Contour::from_bulge_vertices(
            &[
                (p(0.0, 0.0), 0.0),
                (p(0.0, 0.0), 0.0),
                (p(1.0, 0.0), 0.3),
                (p(1.0, 0.0), 0.0),
                (p(1.0, 1.0), 0.0),
                (p(0.0, 1.0), 0.0),
                (p(0.0, 0.0), 0.0),
            ],
            true,
        );
        assert_eq!(c.segments.len(), 4);
        assert!(matches!(c.segments[1], Segment::Line { .. }));
        assert!(c.segments.iter().all(|s| s.length() > 0.5));
        assert!((c.signed_area() - 1.0).abs() < T);
        let open = Contour::from_bulge_vertices(&[(p(2.0, 2.0), 1.0), (p(2.0, 2.0), 0.0)], false);
        assert!(open.segments.is_empty());
    }

    #[test]
    fn test_21b_reverse_twice_is_original() {
        let orig = half_disc();
        let mut c = orig.clone();
        c.reverse();
        assert!(c.start().approx_eq(orig.end(), T));
        c.reverse();
        assert_eq!(c.segments.len(), orig.segments.len());
        for (a, b) in c.segments.iter().zip(&orig.segments) {
            assert!(a.start().approx_eq(b.start(), T) && a.end().approx_eq(b.end(), T));
            assert!((a.length() - b.length()).abs() < T);
        }
    }
}
