//! Rounding sharp corners of a contour with tangent arcs (M16).

use crate::{Contour, Point, Segment, EPS};

/// Joints turning by this angle or less are not corners (radians, = 1°).
pub const FILLET_MIN_TURN: f64 = 0.017453292519943295;
/// Fit tolerance (mm); a line shortened below this is dropped.
pub const FILLET_FIT_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilletSkip {
    /// r is not finite or r < EPS.
    BadRadius,
    /// No such joint, a tangent / almost straight joint (|θ| <= FILLET_MIN_TURN) or a zero-length side.
    NotSharp,
    /// Sharp, but one side is an arc.
    NotLines,
    /// The tangent distance does not fit; `max_r` is the largest radius that would.
    TooLarge { max_r: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilletAll {
    pub contour: Contour,
    pub rounded: usize,
    pub not_lines: usize,
    pub too_large: usize,
}

/// Every joint (index, vertex): closed 0..n, open 1..n (joint k is between segment k-1 (mod n) and k, at
/// segments[k].start()). Contours with fewer than 2 segments have none.
pub fn joints(c: &Contour) -> Vec<(usize, Point)> {
    let n = c.segments.len();
    if n < 2 {
        return Vec::new();
    }
    let from = if c.closed { 0 } else { 1 };
    (from..n).map(|k| (k, c.segments[k].start())).collect()
}

fn start_dir(s: &Segment) -> Option<Point> {
    match *s {
        Segment::Line { start, end } => {
            let l = start.distance(end);
            (l >= EPS).then(|| (end - start) * (1.0 / l))
        }
        Segment::Arc {
            start_angle, sweep, ..
        } => Some(Point::new(-start_angle.sin(), start_angle.cos()) * sweep.signum()),
    }
}

fn end_dir(s: &Segment) -> Option<Point> {
    match *s {
        Segment::Line { .. } => start_dir(s),
        Segment::Arc {
            start_angle, sweep, ..
        } => {
            let a = start_angle + sweep;
            Some(Point::new(-a.sin(), a.cos()) * sweep.signum())
        }
    }
}

/// Signed turn angle at the joint (> 0 = left turn).
fn turn(prev: &Segment, next: &Segment) -> Option<f64> {
    let (u1, u2) = (end_dir(prev)?, start_dir(next)?);
    Some(u1.cross(u2).atan2(u1.dot(u2)))
}

/// `(t, t1, t2, arc)`: tangent distance, tangent points on the incoming / outgoing line and the arc.
fn corner_arc(v: Point, u1: Point, u2: Point, theta: f64, r: f64) -> (f64, Point, Point, Segment) {
    let t = r * (theta.abs() / 2.0).tan();
    let (t1, t2) = (v - u1 * t, v + u2 * t);
    let n = if theta > 0.0 {
        Point::new(-u1.y, u1.x)
    } else {
        Point::new(u1.y, -u1.x)
    };
    let center = t1 + n * r;
    let start_angle = (t1.y - center.y).atan2(t1.x - center.x);
    let arc = Segment::Arc {
        center,
        radius: r,
        start_angle,
        sweep: theta,
    };
    (t, t1, t2, arc)
}

type Fillets = Vec<Result<Segment, FilletSkip>>;

/// Shared core: one result per entry of `which` (ascending joint indices), greedy in that order.
fn fillet_joints(c: &Contour, which: &[usize], r: f64) -> (Contour, Fillets) {
    if !(r.is_finite() && r >= EPS) {
        return (c.clone(), vec![Err(FilletSkip::BadRadius); which.len()]);
    }
    let n = c.segments.len();
    let valid = joints(c);
    let mut cs = vec![0.0; n];
    let mut ce = vec![0.0; n];
    let mut arc_at: Vec<Option<(Point, Point, Segment)>> = vec![None; n];
    let mut results = Vec::with_capacity(which.len());
    for &k in which {
        if !valid.iter().any(|&(j, _)| j == k) {
            results.push(Err(FilletSkip::NotSharp));
            continue;
        }
        let (prev, next) = ((k + n - 1) % n, k);
        let (sp, sn) = (&c.segments[prev], &c.segments[next]);
        let v = sn.start();
        let theta = match turn(sp, sn) {
            Some(t) if t.abs() > FILLET_MIN_TURN => t,
            _ => {
                results.push(Err(FilletSkip::NotSharp));
                continue;
            }
        };
        if !matches!(sp, Segment::Line { .. }) || !matches!(sn, Segment::Line { .. }) {
            results.push(Err(FilletSkip::NotLines));
            continue;
        }
        let avail_p = sp.length() - cs[prev] - ce[prev];
        let avail_n = sn.length() - cs[next] - ce[next];
        let half_tan = (theta.abs() / 2.0).tan();
        let t = r * half_tan;
        if !t.is_finite() || t > avail_p + FILLET_FIT_TOL || t > avail_n + FILLET_FIT_TOL {
            let max_r = avail_p.min(avail_n).max(0.0) / half_tan;
            results.push(Err(FilletSkip::TooLarge { max_r }));
            continue;
        }
        let (u1, u2) = (end_dir(sp).unwrap(), start_dir(sn).unwrap());
        let (t, t1, t2, arc) = corner_arc(v, u1, u2, theta, r);
        ce[prev] = t;
        cs[next] = t;
        arc_at[k] = Some((t1, t2, arc));
        results.push(Ok(arc));
    }
    let mut segments = Vec::new();
    for (i, seg) in c.segments.iter().enumerate() {
        let after = (i + 1) % n;
        let has_after = c.closed || i + 1 < n;
        match *seg {
            Segment::Line { start, end } => {
                if seg.length() - cs[i] - ce[i] >= FILLET_FIT_TOL {
                    segments.push(Segment::Line {
                        start: arc_at[i].map_or(start, |a| a.1),
                        end: if has_after {
                            arc_at[after].map_or(end, |a| a.0)
                        } else {
                            end
                        },
                    });
                }
            }
            arc => segments.push(arc),
        }
        if has_after {
            if let Some((_, _, arc)) = arc_at[after] {
                segments.push(arc);
            }
        }
    }
    (
        Contour {
            segments,
            closed: c.closed,
        },
        results,
    )
}

/// Rounds joint `joint` with radius `r` using the full current lengths of its two lines.
/// Ok = (new contour, the new arc). Err leaves nothing changed.
pub fn fillet_corner(c: &Contour, joint: usize, r: f64) -> Result<(Contour, Segment), FilletSkip> {
    if !r.is_finite() || r < EPS {
        return Err(FilletSkip::BadRadius);
    }
    let (new, res) = fillet_joints(c, &[joint], r);
    res[0].map(|arc| (new, arc))
}

/// Rounds every sharp joint greedily in joint order. NotSharp joints are not counted.
/// BadRadius: the input unchanged, all counts 0.
pub fn fillet_all(c: &Contour, r: f64) -> FilletAll {
    let which: Vec<usize> = joints(c).into_iter().map(|(k, _)| k).collect();
    let (contour, res) = fillet_joints(c, &which, r);
    let mut out = FilletAll {
        contour,
        rounded: 0,
        not_lines: 0,
        too_large: 0,
    };
    for x in res {
        match x {
            Ok(_) => out.rounded += 1,
            Err(FilletSkip::NotLines) => out.not_lines += 1,
            Err(FilletSkip::TooLarge { .. }) => out.too_large += 1,
            Err(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::{self, Primitive};
    use crate::Orientation;
    use std::f64::consts::{FRAC_PI_2, PI};

    const T: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn sharp() -> Contour {
        shapes::rect(p(0.0, 0.0), 30.0, 10.0, 0.0).unwrap()
    }

    fn lines(pts: &[(f64, f64)], closed: bool) -> Contour {
        let n = if closed { pts.len() } else { pts.len() - 1 };
        Contour {
            segments: (0..n)
                .map(|i| Segment::Line {
                    start: p(pts[i].0, pts[i].1),
                    end: p(pts[(i + 1) % pts.len()].0, pts[(i + 1) % pts.len()].1),
                })
                .collect(),
            closed,
        }
    }

    fn same(a: &Contour, b: &Contour) {
        assert_eq!(a.segments.len(), b.segments.len());
        assert_eq!(a.closed, b.closed);
        for (x, y) in a.segments.iter().zip(&b.segments) {
            assert!(x.start().approx_eq(y.start(), T), "{x:?} {y:?}");
            assert!(x.end().approx_eq(y.end(), T), "{x:?} {y:?}");
            assert!(x.midpoint().approx_eq(y.midpoint(), T), "{x:?} {y:?}");
            match (x, y) {
                (Segment::Line { .. }, Segment::Line { .. }) => {}
                (
                    Segment::Arc {
                        center: c1,
                        radius: r1,
                        sweep: w1,
                        ..
                    },
                    Segment::Arc {
                        center: c2,
                        radius: r2,
                        sweep: w2,
                        ..
                    },
                ) => {
                    assert!(c1.approx_eq(*c2, T));
                    assert!((r1 - r2).abs() < T && (w1 - w2).abs() < T);
                }
                _ => panic!("variant {x:?} {y:?}"),
            }
        }
    }

    fn assert_arc(s: &Segment, c: Point, r: f64, sweep: f64, a: Point, b: Point) {
        let Segment::Arc {
            center,
            radius,
            sweep: w,
            ..
        } = *s
        else {
            panic!("not an arc {s:?}")
        };
        assert!(center.approx_eq(c, T) && (radius - r).abs() < T && (w - sweep).abs() < T);
        assert!(
            s.start().approx_eq(a, T) && s.end().approx_eq(b, T),
            "{s:?}"
        );
    }

    fn connected(c: &Contour) {
        let n = c.segments.len();
        for i in 0..n {
            if i + 1 < n || c.closed {
                let (e, s) = (c.segments[i].end(), c.segments[(i + 1) % n].start());
                assert!(e.approx_eq(s, T), "gap at {i}: {e:?} {s:?}");
            }
        }
    }

    #[test]
    fn g1_full_rectangle_is_rounded_rect() {
        let f = fillet_all(&sharp(), 2.0);
        assert_eq!((f.rounded, f.not_lines, f.too_large), (4, 0, 0));
        same(
            &f.contour,
            &shapes::rect(p(0.0, 0.0), 30.0, 10.0, 2.0).unwrap(),
        );
        match shapes::recognize(&f.contour) {
            Some(Primitive::Rect { min, w, h, r }) => {
                assert!(min.approx_eq(p(0.0, 0.0), 1e-6));
                assert!(
                    (w - 30.0).abs() < 1e-6 && (h - 10.0).abs() < 1e-6 && (r - 2.0).abs() < 1e-6
                );
            }
            o => panic!("{o:?}"),
        }
        assert!((f.contour.signed_area() - (284.0 + 4.0 * PI)).abs() < T);
        assert!((f.contour.length() - (64.0 + 4.0 * PI)).abs() < T);
    }

    #[test]
    fn g2_single_corner() {
        let (c, arc) = fillet_corner(&sharp(), 1, 2.0).unwrap();
        assert_eq!(c.segments.len(), 5);
        let l = |a: (f64, f64), b: (f64, f64)| Segment::Line {
            start: p(a.0, a.1),
            end: p(b.0, b.1),
        };
        same(
            &c,
            &Contour {
                segments: vec![
                    l((0.0, 0.0), (28.0, 0.0)),
                    arc,
                    l((30.0, 2.0), (30.0, 10.0)),
                    l((30.0, 10.0), (0.0, 10.0)),
                    l((0.0, 10.0), (0.0, 0.0)),
                ],
                closed: true,
            },
        );
        assert_arc(
            &c.segments[1],
            p(28.0, 2.0),
            2.0,
            FRAC_PI_2,
            p(28.0, 0.0),
            p(30.0, 2.0),
        );
        assert_eq!(c.segments[1], arc);
        assert!((c.length() - (76.0 + PI)).abs() < T);
    }

    #[test]
    fn g3_joint_zero() {
        let (c, _) = fillet_corner(&sharp(), 0, 2.0).unwrap();
        assert_eq!(c.segments.len(), 5);
        assert!(c.segments[0].start().approx_eq(p(2.0, 0.0), T));
        assert!(c.segments[0].end().approx_eq(p(30.0, 0.0), T));
        assert_arc(
            &c.segments[4],
            p(2.0, 2.0),
            2.0,
            FRAC_PI_2,
            p(0.0, 2.0),
            p(2.0, 0.0),
        );
        connected(&c);
        let Segment::Line { start, end } = c.segments[3] else {
            panic!()
        };
        assert!(start.approx_eq(p(0.0, 10.0), T) && end.approx_eq(p(0.0, 2.0), T));
    }

    #[test]
    fn g4_sixty_degrees_open() {
        let y = 8.660254037844386;
        let c = lines(&[(0.0, 0.0), (10.0, 0.0), (15.0, y)], false);
        let (n, _) = fillet_corner(&c, 1, 3.0).unwrap();
        assert_eq!(n.segments.len(), 3);
        assert!(n.segments[0].end().approx_eq(p(8.267949192431123, 0.0), T));
        assert_arc(
            &n.segments[1],
            p(8.267949192431123, 3.0),
            3.0,
            PI / 3.0,
            p(8.267949192431123, 0.0),
            p(10.866_025_403_784_44, 1.5),
        );
        assert!(n.segments[2].end().approx_eq(p(15.0, y), T));
        assert!(!n.closed && n.start() == c.start());
        connected(&n);
    }

    #[test]
    fn g5_concave_corner() {
        let l = lines(
            &[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 10.0),
                (10.0, 10.0),
                (10.0, 20.0),
                (0.0, 20.0),
            ],
            true,
        );
        assert_eq!(l.segments.len(), 6);
        let (c, arc) = fillet_corner(&l, 3, 2.0).unwrap();
        assert_arc(
            &arc,
            p(12.0, 12.0),
            2.0,
            -FRAC_PI_2,
            p(12.0, 10.0),
            p(10.0, 12.0),
        );
        assert!((c.signed_area() - (300.0 + 4.0 - PI)).abs() < T);
        let f = fillet_all(&l, 2.0);
        assert_eq!(f.rounded, 6);
        assert_eq!(f.contour.orientation(), Some(Orientation::Ccw));
        connected(&f.contour);
    }

    #[test]
    fn g6_exact_fit_drops_lines() {
        let f = fillet_all(&sharp(), 5.0);
        assert_eq!(f.rounded, 4);
        let kinds: Vec<bool> = f
            .contour
            .segments
            .iter()
            .map(|s| matches!(s, Segment::Line { .. }))
            .collect();
        assert_eq!(kinds, [true, false, false, true, false, false]);
        connected(&f.contour);
        assert!(shapes::recognize(&f.contour).is_none());
    }

    #[test]
    fn g7_greedy_skip() {
        let f = fillet_all(&sharp(), 6.0);
        assert_eq!((f.rounded, f.too_large), (2, 2));
        assert_eq!(f.contour.segments.len(), 6);
        let lines: Vec<&Segment> = f
            .contour
            .segments
            .iter()
            .filter(|s| matches!(s, Segment::Line { .. }))
            .collect();
        assert_eq!(lines.len(), 4);
        assert!(lines
            .iter()
            .any(|s| s.start().approx_eq(p(30.0, 6.0), T) && s.end().approx_eq(p(30.0, 10.0), T)));
        assert!(lines
            .iter()
            .any(|s| s.start().approx_eq(p(0.0, 10.0), T) && s.end().approx_eq(p(0.0, 6.0), T)));
        connected(&f.contour);
    }

    #[test]
    fn g8_too_large_single() {
        match fillet_corner(&sharp(), 1, 11.0) {
            Err(FilletSkip::TooLarge { max_r }) => assert!((max_r - 10.0).abs() < T),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn g9_not_lines() {
        let d = Contour {
            segments: vec![
                Segment::Line {
                    start: p(0.0, 0.0),
                    end: p(10.0, 0.0),
                },
                Segment::Arc {
                    center: p(5.0, 0.0),
                    radius: 5.0,
                    start_angle: 0.0,
                    sweep: PI,
                },
            ],
            closed: true,
        };
        let f = fillet_all(&d, 1.0);
        assert_eq!((f.rounded, f.not_lines, f.too_large), (0, 2, 0));
        assert_eq!(f.contour, d);
        assert_eq!(fillet_corner(&d, 1, 1.0).unwrap_err(), FilletSkip::NotLines);
    }

    #[test]
    fn g10_smooth_joints() {
        let rr = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 2.0).unwrap();
        let f = fillet_all(&rr, 1.0);
        assert_eq!((f.rounded, f.not_lines, f.too_large), (0, 0, 0));
        assert_eq!(f.contour, rr);
        let circ = shapes::circle(p(0.0, 0.0), 5.0).unwrap();
        assert!(joints(&circ).is_empty());
        let f = fillet_all(&circ, 1.0);
        assert_eq!((f.rounded, f.not_lines, f.too_large), (0, 0, 0));
        let flat = lines(
            &[
                (0.0, 0.0),
                (10.0, 0.0),
                (20.0, 10.0 * 0.5_f64.to_radians().tan()),
            ],
            false,
        );
        assert_eq!(
            fillet_corner(&flat, 1, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        let bent = lines(
            &[
                (0.0, 0.0),
                (10.0, 0.0),
                (20.0, 10.0 * 2.0_f64.to_radians().tan()),
            ],
            false,
        );
        assert!(fillet_corner(&bent, 1, 1.0).is_ok());
    }

    #[test]
    fn g11_open_ends() {
        let c = lines(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false);
        assert_eq!(joints(&c), vec![(1, p(10.0, 0.0))]);
        let f = fillet_all(&c, 2.0);
        assert_eq!(f.rounded, 1);
        assert!(f.contour.start().approx_eq(p(0.0, 0.0), T));
        assert!(f.contour.end().approx_eq(p(10.0, 10.0), T));
        assert_eq!(fillet_corner(&c, 0, 2.0).unwrap_err(), FilletSkip::NotSharp);
    }

    #[test]
    fn g12_cw_contour() {
        let mut c = sharp();
        c.reverse();
        let f = fillet_all(&c, 2.0);
        assert_eq!(f.rounded, 4);
        assert_eq!(f.contour.orientation(), Some(Orientation::Cw));
        for s in &f.contour.segments {
            if let Segment::Arc { sweep, .. } = s {
                assert!((sweep + FRAC_PI_2).abs() < T);
            }
        }
        assert!(matches!(
            shapes::recognize(&f.contour),
            Some(Primitive::Rect { r, .. }) if (r - 2.0).abs() < 1e-6
        ));
    }

    #[test]
    fn g13_bad_radius() {
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                fillet_corner(&sharp(), 1, r).unwrap_err(),
                FilletSkip::BadRadius
            );
            let f = fillet_all(&sharp(), r);
            assert_eq!(f.contour, sharp());
            assert_eq!((f.rounded, f.not_lines, f.too_large), (0, 0, 0));
        }
    }

    // ---- verifier additions ----

    fn invariants(orig: &Contour, f: &Contour) {
        connected(f);
        assert_eq!(f.closed, orig.closed);
        if orig.closed {
            assert_eq!(f.orientation(), orig.orientation());
        }
        assert!(f.length() <= orig.length() + T, "rounding must shorten");
        for s in &f.segments {
            if let Segment::Arc {
                center,
                radius,
                sweep,
                ..
            } = *s
            {
                assert!(sweep.abs() > 0.0 && sweep.abs() < PI);
                assert!((s.start().distance(center) - radius).abs() < 1e-8);
                assert!((s.end().distance(center) - radius).abs() < 1e-8);
            }
        }
    }

    fn polys() -> Vec<Contour> {
        let l = lines(
            &[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 10.0),
                (10.0, 10.0),
                (10.0, 20.0),
                (0.0, 20.0),
            ],
            true,
        );
        let tri = lines(&[(0.0, 0.0), (40.0, 0.0), (10.0, 5.0)], true);
        let mut cw = l.clone();
        cw.reverse();
        let mut tri_cw = tri.clone();
        tri_cw.reverse();
        vec![l, cw, tri, tri_cw, sharp()]
    }

    #[test]
    fn v_invariants_many_radii_both_orientations() {
        for c in polys() {
            for r in [1e-4, 0.01, 0.5, 1.0, 2.0, 3.0, 4.9, 100.0] {
                let f = fillet_all(&c, r);
                invariants(&c, &f.contour);
                if r <= 0.01 {
                    assert_eq!(f.rounded, c.segments.len(), "r={r}");
                }
            }
        }
    }

    #[test]
    fn v_cw_arc_signs_follow_turn() {
        // CW L-shape: convex corners get CW arcs (-), the concave one CCW (+)
        let mut l = polys().remove(0);
        l.reverse();
        let f = fillet_all(&l, 1.0);
        let neg = f
            .contour
            .segments
            .iter()
            .filter(|s| matches!(s, Segment::Arc { sweep, .. } if *sweep < 0.0))
            .count();
        let pos = f.contour.segments.len()
            - neg
            - f.contour
                .segments
                .iter()
                .filter(|s| matches!(s, Segment::Line { .. }))
                .count();
        assert_eq!((neg, pos), (5, 1));
    }

    #[test]
    fn v_triangle_acute_angle_fit() {
        // angle at (0,0) is atan(5/10)? vertices (0,0),(40,0),(10,5): joint 1 is acute
        let tri = lines(&[(0.0, 0.0), (40.0, 0.0), (10.0, 5.0)], true);
        let f = fillet_all(&tri, 1.0);
        assert_eq!(f.rounded, 3);
        invariants(&tri, &f.contour);
        // inscribed: huge r is skipped everywhere, never panics
        let g = fillet_all(&tri, 50.0);
        assert_eq!(g.rounded, 0);
        assert_eq!(g.too_large, 3);
        assert_eq!(g.contour, tri);
    }

    #[test]
    fn v_exact_fit_limit_and_just_above() {
        let c = sharp();
        assert!(fillet_corner(&c, 1, 10.0).is_ok());
        assert!(fillet_corner(&c, 1, 10.0 + 5e-7).is_ok()); // inside FILLET_FIT_TOL
        match fillet_corner(&c, 1, 10.0 + 1e-5) {
            Err(FilletSkip::TooLarge { max_r }) => assert!((max_r - 10.0).abs() < T),
            o => panic!("{o:?}"),
        }
        // joint 1 with r=10 consumes the whole right side; neighbour joint 2 then sees nothing
        let f = fillet_all(&c, 10.0);
        assert_eq!(f.rounded + f.too_large, 4);
        invariants(&c, &f.contour);
    }

    #[test]
    fn v_square_r_half_side_is_circle_of_arcs() {
        let sq = shapes::rect(p(0.0, 0.0), 10.0, 10.0, 0.0).unwrap();
        let f = fillet_all(&sq, 5.0);
        assert_eq!(f.rounded, 4);
        assert_eq!(f.contour.segments.len(), 4);
        assert!(f
            .contour
            .segments
            .iter()
            .all(|s| matches!(s, Segment::Arc { .. })));
        invariants(&sq, &f.contour);
        assert!((f.contour.length() - 10.0 * PI).abs() < 1e-8);
    }

    #[test]
    fn v_slot_matches_shapes_rect_and_recognize_consistent() {
        let f = fillet_all(&sharp(), 5.0);
        let r = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 5.0).unwrap();
        same(&f.contour, &r);
        assert_eq!(shapes::recognize(&f.contour), shapes::recognize(&r));
    }

    #[test]
    fn v_near_straight_threshold() {
        let mk = |deg: f64| {
            lines(
                &[
                    (0.0, 0.0),
                    (10.0, 0.0),
                    (20.0, 10.0 * deg.to_radians().tan()),
                ],
                false,
            )
        };
        assert_eq!(
            fillet_corner(&mk(0.99), 1, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        assert_eq!(
            fillet_corner(&mk(-0.99), 1, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        assert!(fillet_corner(&mk(1.01), 1, 0.1).is_ok());
        assert!(fillet_corner(&mk(-1.01), 1, 0.1).is_ok());
        let f = fillet_all(&mk(0.5), 1.0);
        assert_eq!((f.rounded, f.too_large, f.not_lines), (0, 0, 0));
        assert_eq!(f.contour, mk(0.5));
    }

    #[test]
    fn v_extreme_radii_no_panic() {
        let c = sharp();
        let tiny = fillet_all(&c, 1e-4);
        assert_eq!(tiny.rounded, 4);
        invariants(&c, &tiny.contour);
        for r in [1e9, 1e300, f64::MAX] {
            let f = fillet_all(&c, r);
            assert_eq!((f.rounded, f.too_large), (0, 4), "r={r}");
            assert_eq!(f.contour, c);
        }
        // below EPS is a bad radius
        assert_eq!(
            fillet_corner(&c, 1, EPS / 2.0).unwrap_err(),
            FilletSkip::BadRadius
        );
    }

    #[test]
    fn v_degenerate_inputs() {
        let empty = Contour {
            segments: vec![],
            closed: true,
        };
        assert!(joints(&empty).is_empty());
        assert_eq!(fillet_all(&empty, 1.0).rounded, 0);
        assert_eq!(
            fillet_corner(&empty, 0, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        let one = lines(&[(0.0, 0.0), (5.0, 0.0)], false);
        assert!(joints(&one).is_empty());
        assert_eq!(
            fillet_corner(&one, 0, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        assert_eq!(
            fillet_corner(&sharp(), 99, 1.0).unwrap_err(),
            FilletSkip::NotSharp
        );
        // zero-length side and a 180 degree spike
        let z = lines(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false);
        let f = fillet_all(&z, 1.0);
        invariants(&z, &f.contour);
        let spike = lines(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.0)], false);
        let f = fillet_all(&spike, 1.0);
        assert_eq!(f.rounded, 0);
        assert_eq!(f.contour, spike);
    }

    #[test]
    fn v_joint0_on_open_never_rounded_and_ends_fixed() {
        let c = lines(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], false);
        let f = fillet_all(&c, 2.0);
        assert_eq!(f.rounded, 2);
        assert!(f.contour.start().approx_eq(c.start(), T));
        assert!(f.contour.end().approx_eq(c.end(), T));
        assert!(!f.contour.closed);
        invariants(&c, &f.contour);
    }

    #[test]
    fn v_greedy_two_short_corners_share_line() {
        // closed strip: a 3 long top side between two corners needing t=2 each -> second fails
        let c = lines(&[(0.0, 0.0), (20.0, 0.0), (20.0, 3.0), (0.0, 3.0)], true);
        let f = fillet_all(&c, 2.0);
        // right side 3 long: joint1 uses 2, joint2 needs 2 but only 1 left
        assert_eq!((f.rounded, f.too_large), (2, 2));
        invariants(&c, &f.contour);
    }

    #[test]
    fn v_tangent_arc_joint_skipped_and_line_arc_sharp_reported() {
        // line to arc with a 90 degree kink: NotLines
        let c = Contour {
            segments: vec![
                Segment::Line {
                    start: p(0.0, 0.0),
                    end: p(10.0, 0.0),
                },
                Segment::Arc {
                    center: p(10.0, 5.0),
                    radius: 5.0,
                    start_angle: -FRAC_PI_2,
                    sweep: -PI,
                },
                Segment::Line {
                    start: p(10.0, 10.0),
                    end: p(0.0, 10.0),
                },
            ],
            closed: false,
        };
        let f = fillet_all(&c, 1.0);
        assert_eq!(f.rounded, 0);
        assert_eq!(f.too_large, 0);
        assert_eq!(f.contour, c);
    }
}
