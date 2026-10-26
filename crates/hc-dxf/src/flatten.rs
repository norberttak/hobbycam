//! Curves that hc-geom cannot represent exactly (ellipse, spline, arcs under non-uniform scale) → points.

use hc_geom::Point;

/// Number of chords for a sweep `dt` (radians) on a curve with max radius `r` so the sagitta stays ≤ tol.
pub fn chord_count(dt: f64, r: f64, tol: f64) -> usize {
    let step = 2.0 * (1.0 - tol / r).max(-1.0).acos();
    let n = if step > 0.0 {
        (dt.abs() / step).ceil()
    } else {
        4.0
    };
    (n as usize).clamp(4, 100_000)
}

/// Points c + major·cos t + minor·sin t for t = t0 .. t0+dt (n+1 points, uniform in t).
pub fn ellipse_points(
    c: Point,
    major: Point,
    minor: Point,
    t0: f64,
    dt: f64,
    tol: f64,
) -> Vec<Point> {
    let n = chord_count(dt, major.length(), tol);
    (0..=n)
        .map(|i| {
            let t = t0 + dt * i as f64 / n as f64;
            c + major * t.cos() + minor * t.sin()
        })
        .collect()
}

/// Arc as points; first/last are the exact arc endpoints.
pub fn arc_points(c: Point, r: f64, start_angle: f64, sweep: f64, tol: f64) -> Vec<Point> {
    let a = Point::new(r, 0.0);
    let b = Point::new(0.0, r);
    let mut pts = ellipse_points(c, a, b, start_angle, sweep, tol);
    let at = |ang: f64| c + Point::new(ang.cos(), ang.sin()) * r;
    let last = pts.len() - 1;
    pts[0] = at(start_angle);
    pts[last] = at(start_angle + sweep);
    pts
}

fn dist_to_chord(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

struct Nurbs<'a> {
    degree: usize,
    ctrl: Vec<(f64, f64, f64)>, // homogeneous (w·x, w·y, w)
    knots: &'a [f64],
}

impl Nurbs<'_> {
    /// de Boor evaluation at u inside knot span k.
    fn eval(&self, u: f64, k: usize) -> Point {
        let p = self.degree;
        let mut d: Vec<(f64, f64, f64)> = (0..=p).map(|j| self.ctrl[j + k - p]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let lo = self.knots[j + k - p];
                let hi = self.knots[j + 1 + k - r];
                let alpha = if hi > lo { (u - lo) / (hi - lo) } else { 0.0 };
                let (a, b) = (d[j - 1], d[j]);
                d[j] = (
                    (1.0 - alpha) * a.0 + alpha * b.0,
                    (1.0 - alpha) * a.1 + alpha * b.1,
                    (1.0 - alpha) * a.2 + alpha * b.2,
                );
            }
        }
        Point::new(d[p].0 / d[p].2, d[p].1 / d[p].2)
    }

    #[allow(clippy::too_many_arguments)]
    fn bisect(
        &self,
        k: usize,
        u0: f64,
        p0: Point,
        u1: f64,
        p1: Point,
        tol: f64,
        depth: u32,
        out: &mut Vec<Point>,
    ) {
        let um = 0.5 * (u0 + u1);
        let pm = self.eval(um, k);
        if depth < 10 && dist_to_chord(pm, p0, p1) > tol {
            self.bisect(k, u0, p0, um, pm, tol, depth + 1, out);
            self.bisect(k, um, pm, u1, p1, tol, depth + 1, out);
        } else {
            out.push(p1);
        }
    }
}

/// Rational B-spline → polyline points. None when the definition is inconsistent.
pub fn spline_points(
    degree: usize,
    ctrl: &[Point],
    knots: &[f64],
    weights: &[f64],
    tol: f64,
) -> Option<Vec<Point>> {
    let n = ctrl.len();
    if degree < 1
        || n < degree + 1
        || knots.len() != n + degree + 1
        || knots.windows(2).any(|w| w[1] < w[0])
    {
        return None;
    }
    let w = |i: usize| if weights.len() == n { weights[i] } else { 1.0 };
    if (0..n).any(|i| w(i) <= 0.0) {
        return None;
    }
    let nurbs = Nurbs {
        degree,
        ctrl: (0..n)
            .map(|i| (ctrl[i].x * w(i), ctrl[i].y * w(i), w(i)))
            .collect(),
        knots,
    };
    let mut out: Vec<Point> = Vec::new();
    for k in degree..n {
        let (u0, u1) = (knots[k], knots[k + 1]);
        if u1 <= u0 {
            continue;
        }
        let p0 = nurbs.eval(u0, k);
        if out.is_empty() {
            out.push(p0);
        }
        let p1 = nurbs.eval(u1, k);
        nurbs.bisect(k, u0, p0, u1, p1, tol, 0, &mut out);
    }
    (out.len() >= 2).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn chord_count_respects_tolerance() {
        // r=10, tol=0.01: step = 2·acos(0.999) ≈ 0.0894 → full circle needs 71 chords
        assert_eq!(chord_count(TAU, 10.0, 0.01), 71);
        assert_eq!(chord_count(0.001, 10.0, 0.01), 4);
        assert_eq!(chord_count(TAU, 0.001, 0.01), 4); // tol larger than radius
    }

    #[test]
    fn arc_points_exact_ends() {
        let pts = arc_points(Point::new(1.0, 1.0), 2.0, 0.0, std::f64::consts::PI, 0.01);
        assert!(pts[0].approx_eq(Point::new(3.0, 1.0), 1e-12));
        assert!(pts[pts.len() - 1].approx_eq(Point::new(-1.0, 1.0), 1e-9));
        assert!(pts
            .iter()
            .all(|p| (p.distance(Point::new(1.0, 1.0)) - 2.0).abs() < 1e-9));
    }

    #[test]
    fn spline_rejects_bad_knots_and_handles_weights() {
        let c = [
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 0.0),
        ];
        assert!(spline_points(2, &c, &[0.0, 0.0, 1.0, 1.0], &[], 0.01).is_none());
        // quarter circle as rational quadratic: weights 1, √2/2, 1
        let c = [
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ];
        let w = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
        let pts = spline_points(2, &c, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &w, 0.001).unwrap();
        assert!(pts.len() > 3);
        assert!(pts.iter().all(|p| (p.length() - 1.0).abs() < 1e-9));
    }
}
