//! Contour offsets via cavalier_contours (arcs stay arcs).

use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use hc_geom::{Contour, Orientation, Point, Segment, EPS};
use std::f64::consts::PI;

/// Offset results smaller than this (mm²) are treated as vanished.
const MIN_AREA: f64 = 1e-6;
/// Safety cap on pocket levels; `generate_op` rejects stepovers that would need more.
pub(crate) const MAX_LEVELS: usize = 10_000;

/// Every arc with |sweep| > π becomes two equal halves (|bulge| ≤ 1, no full-circle moves).
pub(crate) fn split_large_arcs(c: &Contour) -> Contour {
    let mut segments = Vec::with_capacity(c.segments.len() + 1);
    for s in &c.segments {
        match *s {
            Segment::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } if sweep.abs() > PI + EPS => {
                let half = sweep / 2.0;
                segments.push(Segment::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep: half,
                });
                segments.push(Segment::Arc {
                    center,
                    radius,
                    start_angle: start_angle + half,
                    sweep: half,
                });
            }
            s => segments.push(s),
        }
    }
    Contour {
        segments,
        closed: c.closed,
    }
}

fn to_pline(c: &Contour) -> Polyline<f64> {
    let c = split_large_arcs(c);
    let mut pl = Polyline::with_capacity(c.segments.len() + 1, c.closed);
    for s in &c.segments {
        let p = s.start();
        let bulge = match *s {
            Segment::Line { .. } => 0.0,
            Segment::Arc { sweep, .. } => (sweep / 4.0).tan(),
        };
        pl.add(p.x, p.y, bulge);
    }
    if !c.closed && !c.segments.is_empty() {
        let e = c.end();
        pl.add(e.x, e.y, 0.0);
    }
    pl.set_is_closed(c.closed);
    pl
}

fn from_pline(pl: &Polyline<f64>) -> Contour {
    let v: Vec<(Point, f64)> = pl
        .iter_vertexes()
        .map(|v| (Point::new(v.x, v.y), v.bulge))
        .collect();
    Contour::from_bulge_vertices(&v, pl.is_closed())
}

fn big_enough(c: &Contour) -> bool {
    c.signed_area().abs() >= MIN_AREA
}

/// Closed-contour offset. dist > 0 grows, < 0 shrinks, independent of input orientation. Results are CCW. Empty = vanished.
pub fn offset_closed(c: &Contour, dist: f64) -> Vec<Contour> {
    if !c.closed || c.segments.is_empty() {
        return Vec::new();
    }
    let mut ccw = c.clone();
    ccw.set_orientation(Orientation::Ccw);
    // cavalier: positive = left of travel = inward for CCW
    to_pline(&ccw)
        .parallel_offset(-dist)
        .iter()
        .map(from_pline)
        .filter(big_enough)
        .map(|mut r| {
            r.set_orientation(Orientation::Ccw);
            r
        })
        .collect()
}

/// Gap (mm) forced between an island and a wall it comes within the tool radius of.
pub(crate) const ISLAND_CLEARANCE: f64 = 5e-4;

/// True when a sample of `c` (segment ends plus points every <= r/2) lies within `r` of `other`.
fn comes_within(c: &Contour, other: &Contour, r: f64) -> bool {
    let step = r / 2.0;
    c.segments.iter().any(|s| {
        let len = s.length();
        let n = if step > 0.0 {
            (len / step).ceil().max(1.0) as usize
        } else {
            1
        };
        (0..=n).any(|k| other.distance_to(s.point_at(len * k as f64 / n as f64)) <= r)
    })
}

/// The pocket region as a cavalier shape: boundary CCW, islands CW (region on the left), islands near a wall shrunk first.
fn pocket_shape(boundary: &Contour, islands: &[Contour], radius: f64) -> Shape<f64> {
    let mut b = boundary.clone();
    b.set_orientation(Orientation::Ccw);
    // cavalier mis-offsets coincident edges: islands within r of another wall get a tiny gap first
    let islands: Vec<Contour> = islands
        .iter()
        .enumerate()
        .flat_map(|(k, isl)| {
            let near = std::iter::once(boundary)
                .chain(
                    islands
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != k)
                        .map(|(_, o)| o),
                )
                .any(|o| comes_within(isl, o, radius));
            if near {
                offset_closed(isl, -ISLAND_CLEARANCE)
            } else {
                vec![isl.clone()]
            }
        })
        .collect();
    let plines = std::iter::once(to_pline(&b)).chain(islands.iter().map(|i| {
        let mut i = i.clone();
        i.set_orientation(Orientation::Cw);
        to_pline(&i)
    }));
    Shape::from_plines(plines)
}

/// Pocket rings per offset level, outermost level first. Boundaries CCW, island rings CW.
pub fn pocket_rings(
    boundary: &Contour,
    islands: &[Contour],
    radius: f64,
    step: f64,
) -> Vec<Vec<Contour>> {
    let opts = ShapeOffsetOptions::new();
    let mut shape = pocket_shape(boundary, islands, radius).parallel_offset(radius, &opts);
    let mut levels = Vec::new();
    while levels.len() < MAX_LEVELS {
        let level: Vec<Contour> = shape
            .ccw_plines
            .iter()
            .chain(&shape.cw_plines)
            .map(|p| from_pline(&p.polyline))
            .filter(big_enough)
            .collect();
        if level.is_empty() {
            break;
        }
        levels.push(level);
        shape = shape.parallel_offset(step, &opts);
    }
    levels
}

fn shape_area(s: &Shape<f64>) -> f64 {
    s.ccw_plines
        .iter()
        .chain(&s.cw_plines)
        .map(|p| p.polyline.area())
        .sum()
}

/// Opening loss k·(tan(φ/2) − φ/2) of a convex turn φ, capped at 2r² (a sharp spike leaves real material).
fn corner_loss(k: f64, phi: f64, r2: f64) -> f64 {
    (k * ((phi / 2.0).tan() - phi / 2.0)).clamp(0.0, 2.0 * r2)
}

/// Pocket area (mm²) a disk of radius `r` cannot reach, minus what no round tool can reach in sharp corners and tight
/// convex fillets. ≈ 0 for shapes the tool fits everywhere; a neck or spur narrower than 2r adds its area.
pub fn uncut_excess(boundary: &Contour, islands: &[Contour], r: f64) -> f64 {
    let opts = ShapeOffsetOptions::new();
    let shape = pocket_shape(boundary, islands, r);
    let opened = shape.parallel_offset(r, &opts).parallel_offset(-r, &opts);
    let r2 = r * r;
    let mut allow = 0.0;
    for p in shape.ccw_plines.iter().chain(&shape.cw_plines) {
        let c = from_pline(&p.polyline);
        let n = c.segments.len();
        for (i, s) in c.segments.iter().enumerate() {
            // region on the left: a left turn is a convex corner of the region
            let (a, b) = (
                crate::check::tangent(s, true),
                crate::check::tangent(&c.segments[(i + 1) % n], false),
            );
            let phi = a.cross(b).atan2(a.dot(b));
            if phi > 1e-9 {
                allow += corner_loss(r2, phi, r2);
            }
            if let Segment::Arc { radius, sweep, .. } = *s {
                if sweep > 0.0 && radius < r {
                    allow += corner_loss(r2 - radius * radius, sweep, r2);
                }
            }
        }
    }
    shape_area(&shape) - shape_area(&opened) - allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_geom::Circle;

    fn circle(r: f64) -> Contour {
        Circle {
            center: Point::new(0.0, 0.0),
            radius: r,
        }
        .to_contour()
    }

    #[test]
    fn split_large_arcs_halves_full_circle_only() {
        let c = split_large_arcs(&circle(2.0));
        assert_eq!(c.segments.len(), 2);
        assert!((c.signed_area() - 4.0 * PI).abs() < 1e-9);
        assert!(c.start().approx_eq(c.end(), 1e-9));
        let half = Contour::from_bulge_vertices(
            &[(Point::new(1.0, 0.0), 1.0), (Point::new(-1.0, 0.0), 0.0)],
            true,
        );
        assert_eq!(split_large_arcs(&half), half);
    }

    #[test]
    fn pline_round_trip_open_and_closed() {
        let open = Contour::from_bulge_vertices(
            &[
                (Point::new(0.0, 0.0), 0.0),
                (Point::new(1.0, 0.0), 0.5),
                (Point::new(2.0, 1.0), 0.0),
            ],
            false,
        );
        let pl = to_pline(&open);
        assert_eq!(pl.vertex_count(), 3);
        assert!(!pl.is_closed());
        let back = from_pline(&pl);
        assert_eq!(back.segments.len(), 2);
        assert!(back.end().approx_eq(open.end(), 1e-9));
        assert!((back.length() - open.length()).abs() < 1e-9);
        let full = from_pline(&to_pline(&circle(3.0)));
        assert!(full.closed);
        assert!((full.signed_area() - 9.0 * PI).abs() < 1e-9);
    }

    #[test]
    fn offset_open_contour_is_empty() {
        let mut c = circle(3.0);
        c.closed = false;
        assert!(offset_closed(&c, 1.0).is_empty());
    }

    #[test]
    fn comes_within_samples_along_segments() {
        let sq = |x0: f64, x1: f64| {
            Contour::from_bulge_vertices(
                &[
                    (Point::new(x0, 0.0), 0.0),
                    (Point::new(x1, 0.0), 0.0),
                    (Point::new(x1, 10.0), 0.0),
                    (Point::new(x0, 10.0), 0.0),
                ],
                true,
            )
        };
        assert!(comes_within(&sq(0.0, 10.0), &sq(10.0, 20.0), 1.0));
        assert!(comes_within(&sq(0.0, 10.0), &sq(10.9, 20.0), 1.0));
        assert!(!comes_within(&sq(0.0, 10.0), &sq(11.1, 20.0), 1.0));
    }
}
