//! Ubuntu Light outlines (from `epaint_default_fonts`, Ubuntu Font Licence 1.0): curves flattened to lines (Wang's formula).

use crate::FLATTEN_TOL;
use hc_geom::{containment_tree, Contour, Orientation, Point, Segment, EPS};
use std::sync::OnceLock;
use ttf_parser::{Face, OutlineBuilder};

const MAX_PIECES: f64 = 64.0;

/// Points after `p0` (the last is `p2` exactly) of a quadratic Bézier flattened within `tol`.
pub(crate) fn flatten_quad(p0: Point, p1: Point, p2: Point, tol: f64) -> Vec<Point> {
    let m = (p0 - p1 * 2.0 + p2).length();
    let n = (m / (4.0 * tol)).sqrt().ceil().clamp(1.0, MAX_PIECES) as usize;
    (1..=n)
        .map(|i| {
            if i == n {
                return p2;
            }
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t)
        })
        .collect()
}

/// Points after `p0` (the last is `p3` exactly) of a cubic Bézier flattened within `tol`.
pub(crate) fn flatten_cubic(p0: Point, p1: Point, p2: Point, p3: Point, tol: f64) -> Vec<Point> {
    let m = (p0 - p1 * 2.0 + p2)
        .length()
        .max((p1 - p2 * 2.0 + p3).length());
    let n = (3.0 * m / (4.0 * tol)).sqrt().ceil().clamp(1.0, MAX_PIECES) as usize;
    (1..=n)
        .map(|i| {
            if i == n {
                return p3;
            }
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        })
        .collect()
}

struct Ttf {
    face: Face<'static>,
    /// mm per font unit for a cap height of 1 mm.
    per_mm: f64,
    space: f64,
}

fn ttf() -> &'static Ttf {
    static F: OnceLock<Ttf> = OnceLock::new();
    F.get_or_init(|| {
        let face = Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)
            .expect("bundled Ubuntu Light parses");
        let h = face.glyph_index('H').expect("H");
        let b = face.glyph_bounding_box(h).expect("H outline");
        let space = face
            .glyph_index(' ')
            .and_then(|g| face.glyph_hor_advance(g))
            .expect("space") as f64;
        Ttf {
            face,
            per_mm: 1.0 / (b.y_max as f64 - b.y_min as f64),
            space,
        }
    })
}

/// Space advance, mm.
pub(crate) fn space(height: f64) -> f64 {
    let t = ttf();
    t.space * t.per_mm * height
}

/// Collects closed contours in mm, dropping pieces shorter than EPS.
struct Builder {
    scale: f64,
    origin: Point,
    start: Point,
    cur: Point,
    segs: Vec<Segment>,
    out: Vec<Contour>,
}

impl Builder {
    fn p(&self, x: f32, y: f32) -> Point {
        self.origin + Point::new(x as f64, y as f64) * self.scale
    }

    fn line(&mut self, p: Point) {
        if self.cur.distance(p) >= EPS {
            self.segs.push(Segment::Line {
                start: self.cur,
                end: p,
            });
            self.cur = p;
        }
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = self.p(x, y);
        self.cur = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.line(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        for p in flatten_quad(self.cur, self.p(x1, y1), self.p(x, y), FLATTEN_TOL) {
            self.line(p);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        for q in flatten_cubic(self.cur, c1, c2, p, FLATTEN_TOL) {
            self.line(q);
        }
    }
    fn close(&mut self) {
        if self.segs.is_empty() {
            return;
        }
        let start = self.start;
        self.line(start);
        // a last point within EPS of the start was merged: end exactly at the start
        if let Some(Segment::Line { end, .. }) = self.segs.last_mut() {
            *end = start;
        }
        let segments = std::mem::take(&mut self.segs);
        if segments.len() >= 2 {
            self.out.push(Contour {
                segments,
                closed: true,
            });
        }
        self.cur = start;
    }
}

/// Closed outlines of `c` with the glyph origin at `origin`: outer contours CCW, counters (holes) CW.
pub(crate) fn glyph(c: char, height: f64, origin: Point) -> Option<(Vec<Contour>, f64)> {
    let t = ttf();
    let id = t.face.glyph_index(c)?;
    let scale = t.per_mm * height;
    let mut b = Builder {
        scale,
        origin,
        start: origin,
        cur: origin,
        segs: Vec::new(),
        out: Vec::new(),
    };
    t.face.outline_glyph(id, &mut b); // None for an empty glyph (space)
    b.close();
    let mut contours = b.out;
    for n in containment_tree(&contours) {
        let o = if n.depth % 2 == 0 {
            Orientation::Ccw
        } else {
            Orientation::Cw
        };
        contours[n.contour].set_orientation(o);
    }
    let adv = t.face.glyph_hor_advance(id).unwrap_or(0) as f64 * scale;
    Some((contours, adv))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_at(p: [Point; 3], t: f64) -> Point {
        let u = 1.0 - t;
        p[0] * (u * u) + p[1] * (2.0 * u * t) + p[2] * (t * t)
    }

    fn cubic_at(p: [Point; 4], t: f64) -> Point {
        let u = 1.0 - t;
        p[0] * (u * u * u)
            + p[1] * (3.0 * u * u * t)
            + p[2] * (3.0 * u * t * t)
            + p[3] * (t * t * t)
    }

    /// Largest distance of 100 curve samples to the polyline.
    fn max_dev(start: Point, pts: &[Point], at: impl Fn(f64) -> Point) -> f64 {
        let mut segs = vec![];
        let mut a = start;
        for &b in pts {
            segs.push(Segment::Line { start: a, end: b });
            a = b;
        }
        (0..100)
            .map(|i| {
                let p = at(i as f64 / 99.0);
                segs.iter()
                    .map(|s| s.distance_to(p))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    }

    /// M10b test 27.
    #[test]
    fn flattening_stays_within_tolerance() {
        let q = [
            Point::new(0.0, 0.0),
            Point::new(5.0, 8.0),
            Point::new(10.0, 0.0),
        ];
        let pts = flatten_quad(q[0], q[1], q[2], FLATTEN_TOL);
        assert!(pts.len() > 1);
        assert_eq!(*pts.last().unwrap(), q[2]);
        assert!(max_dev(q[0], &pts, |t| quad_at(q, t)) <= FLATTEN_TOL);

        let c = [
            Point::new(0.0, 0.0),
            Point::new(0.0, 6.0),
            Point::new(10.0, -6.0),
            Point::new(10.0, 0.0),
        ];
        let pts = flatten_cubic(c[0], c[1], c[2], c[3], FLATTEN_TOL);
        assert!(pts.len() > 1);
        assert_eq!(*pts.last().unwrap(), c[3]);
        assert!(max_dev(c[0], &pts, |t| cubic_at(c, t)) <= FLATTEN_TOL);
    }

    #[test]
    fn straight_curve_is_one_piece_and_n_is_capped() {
        let (a, m, b) = (
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
        );
        assert_eq!(flatten_quad(a, m, b, FLATTEN_TOL), vec![b]);
        let c = Point::new(3.0, 3.0);
        assert_eq!(flatten_cubic(a, m, b, c, FLATTEN_TOL), vec![c]);
        let far = Point::new(1e6, 1e6);
        assert_eq!(flatten_quad(a, far, b, FLATTEN_TOL).len(), 64);
    }

    #[test]
    fn h_sits_on_the_baseline() {
        let t = ttf();
        let b = t
            .face
            .glyph_bounding_box(t.face.glyph_index('H').unwrap())
            .unwrap();
        assert_eq!(b.y_min, 0, "`at` is the baseline: H must start at y 0");
    }
}
