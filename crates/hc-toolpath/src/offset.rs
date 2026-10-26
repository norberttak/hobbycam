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

/// Pocket rings per offset level, outermost level first. Boundaries CCW, island rings CW.
pub fn pocket_rings(
    boundary: &Contour,
    islands: &[Contour],
    radius: f64,
    step: f64,
) -> Vec<Vec<Contour>> {
    let mut b = boundary.clone();
    b.set_orientation(Orientation::Ccw);
    let plines = std::iter::once(to_pline(&b)).chain(islands.iter().map(|i| {
        let mut i = i.clone();
        i.set_orientation(Orientation::Cw);
        to_pline(&i)
    }));
    let opts = ShapeOffsetOptions::new();
    let mut shape = Shape::from_plines(plines).parallel_offset(radius, &opts);
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
}
