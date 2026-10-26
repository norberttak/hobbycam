use crate::{Contour, Point, Segment, EPS};
use std::f64::consts::PI;

/// Nearest unused segment with an endpoint within `tol` of `at`.
/// Returns (index, matched_at_its_start). Ties: lowest index, then start before end.
fn nearest(segments: &[Segment], used: &[bool], at: Point, tol: f64) -> Option<(usize, bool)> {
    let mut best: Option<(f64, usize, bool)> = None;
    for (i, s) in segments.iter().enumerate() {
        if used[i] {
            continue;
        }
        for (pt, is_start) in [(s.start(), true), (s.end(), false)] {
            let d = pt.distance(at);
            if d <= tol && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, i, is_start));
            }
        }
    }
    best.map(|(_, i, is_start)| (i, is_start))
}

/// Make every joint exact (all are within tol already): move a Line endpoint onto the
/// neighbouring segment, or insert a connector Line between two arcs when the gap > EPS.
fn connect_joints(chain: Vec<Segment>, closed: bool) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::with_capacity(chain.len());
    for seg in chain {
        out.push(seg);
        let n = out.len();
        if n > 1 {
            join(&mut out, n - 2, n - 1);
        }
    }
    if closed {
        let n = out.len();
        join(&mut out, n - 1, 0);
    }
    out
}

/// Fix the joint between out[prev].end and out[next].start, inserting a connector after `prev` if needed.
fn join(out: &mut Vec<Segment>, prev: usize, next: usize) {
    let pe = out[prev].end();
    let ns = out[next].start();
    if let Segment::Line { start, .. } = &mut out[next] {
        *start = pe;
    } else if let Segment::Line { end, .. } = &mut out[prev] {
        *end = ns;
    } else if pe.distance(ns) > EPS {
        out.insert(prev + 1, Segment::Line { start: pe, end: ns });
    }
}

/// Greedily join loose segments into contours (endpoints within `tol` connect).
/// Segments shorter than EPS are dropped. Output order follows each chain's first segment.
/// At junctions where more than two ends meet the greedy pick is accepted as is.
// shortcut: O(n²) search, fine for hobby drawings (< few thousand segments); upgrade to a spatial grid if slow.
pub fn chain_segments(segments: &[Segment], tol: f64) -> Vec<Contour> {
    let segs: Vec<Segment> = segments
        .iter()
        .filter(|s| s.length() >= EPS)
        .copied()
        .collect();
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();

    for first in 0..segs.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut chain = vec![segs[first]];
        // a lone segment closes only as a (near) full arc, never a tiny line/arc shorter than tol
        let is_closed = |c: &[Segment]| match c {
            [Segment::Arc { sweep, .. }] if sweep.abs() > PI => {
                c[0].end().approx_eq(c[0].start(), tol)
            }
            [_] => false,
            _ => c[c.len() - 1].end().approx_eq(c[0].start(), tol),
        };

        // stop growing once closed, so a full circle stays on its own
        while !is_closed(&chain) {
            let end = chain[chain.len() - 1].end();
            if let Some((i, at_start)) = nearest(&segs, &used, end, tol) {
                used[i] = true;
                chain.push(if at_start {
                    segs[i]
                } else {
                    segs[i].reversed()
                });
                continue;
            }
            // extend backwards: candidate must end at the chain start
            if let Some((i, at_start)) = nearest(&segs, &used, chain[0].start(), tol) {
                used[i] = true;
                chain.insert(
                    0,
                    if at_start {
                        segs[i].reversed()
                    } else {
                        segs[i]
                    },
                );
                continue;
            }
            break;
        }

        let closed = is_closed(&chain);
        out.push(Contour {
            segments: connect_joints(chain, closed),
            closed,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Circle, Orientation};

    const T: f64 = 1e-9;

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Segment {
        Segment::Line {
            start: Point::new(x0, y0),
            end: Point::new(x1, y1),
        }
    }

    fn square_lines(x: f64, y: f64, s: f64) -> Vec<Segment> {
        vec![
            line(x, y, x + s, y),
            line(x + s, y, x + s, y + s),
            line(x + s, y + s, x, y + s),
            line(x, y + s, x, y),
        ]
    }

    #[test]
    fn test_03_shuffled_and_reversed_square() {
        let sq = square_lines(0.0, 0.0, 1.0);
        let input = [sq[2], sq[0].reversed(), sq[3], sq[1].reversed()];
        let out = chain_segments(&input, 0.01);
        assert_eq!(out.len(), 1);
        assert!(out[0].closed);
        assert_eq!(out[0].segments.len(), 4);
        assert!((out[0].signed_area().abs() - 1.0).abs() < T);
    }

    #[test]
    fn test_04_tolerance() {
        let input = [
            line(0.0, 0.0, 1.0, 0.0),
            line(1.005, 0.0, 1.0, 1.0),
            line(1.0, 1.005, 0.0, 1.0),
            line(0.0, 1.0, 0.0, 0.005),
        ];
        let out = chain_segments(&input, 0.01);
        assert_eq!(out.len(), 1);
        assert!(out[0].closed);
        // closing line snapped exactly onto the start
        assert_eq!(out[0].end(), out[0].start());

        let out = chain_segments(&input, 0.001);
        assert!(out.len() > 1);
        assert!(out.iter().all(|c| !c.closed));
    }

    #[test]
    fn test_05_open_polyline() {
        let out = chain_segments(&[line(0.0, 0.0, 1.0, 0.0), line(1.0, 0.0, 1.0, 1.0)], 0.01);
        assert_eq!(out.len(), 1);
        assert!(!out[0].closed);
        assert_eq!(out[0].orientation(), None);
        assert_eq!(out[0].signed_area(), 0.0);
    }

    #[test]
    fn test_06_two_disjoint_squares() {
        let a = square_lines(0.0, 0.0, 1.0);
        let b = square_lines(5.0, 5.0, 2.0);
        let input = [b[1], a[3], a[0], b[3], b[0], a[2], a[1], b[2]];
        let out = chain_segments(&input, 0.01);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| c.closed));
        // output order follows first segment: square b first
        assert!((out[0].signed_area().abs() - 4.0).abs() < T);
        assert!((out[1].signed_area().abs() - 1.0).abs() < T);
    }

    #[test]
    fn a2_joints_connected_and_arc_connector() {
        let input = [
            line(0.0, 0.0, 10.0, 0.0),
            line(10.005, 0.0, 10.0, 10.0),
            line(10.0, 10.0, 0.0, 10.0),
            line(0.0, 10.0, 0.0, 0.005),
        ];
        let c = &chain_segments(&input, 0.01)[0];
        let n = c.segments.len();
        for i in 0..n {
            assert_eq!(c.segments[i].end(), c.segments[(i + 1) % n].start());
        }
        // two arcs with a 0.005 gap get a connector line
        let a1 = Segment::Arc {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        let a2 = Segment::Arc {
            center: Point::new(0.005, 2.0),
            radius: 1.0,
            start_angle: -std::f64::consts::FRAC_PI_2,
            sweep: -std::f64::consts::FRAC_PI_2,
        };
        let out = chain_segments(&[a1, a2], 0.01);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].segments.len(), 3);
        assert!(matches!(out[0].segments[1], Segment::Line { .. }));
        assert_eq!(out[0].segments[1].start(), a1.end());
        assert_eq!(out[0].segments[1].end(), a2.start());
    }

    #[test]
    fn r1_tiny_lone_line_is_not_closed() {
        let out = chain_segments(
            &[line(0.0, 0.0, 0.005, 0.0), line(0.005, 0.0, 1.005, 0.0)],
            0.01,
        );
        assert_eq!(out.len(), 1);
        assert!(!out[0].closed);
        assert_eq!(out[0].segments.len(), 2);
        assert!(out[0].segments.iter().all(|s| s.length() > 0.001));
    }

    #[test]
    fn test_22_zero_length_line_dropped() {
        let mut input = square_lines(0.0, 0.0, 1.0);
        input.insert(2, line(1.0, 1.0, 1.0, 1.0));
        let out = chain_segments(&input, 0.01);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].segments.len(), 4);
        assert!(out[0].closed);
    }

    #[test]
    fn full_circle_arc_closed_alone_and_backward_extension() {
        let circle = Circle {
            center: Point::new(1.0, 0.0),
            radius: 1.0,
        }
        .to_contour()
        .segments[0];
        // a line touching the circle start must not get attached to it
        let out = chain_segments(&[circle, line(2.0, 0.0, 3.0, 0.0)], 0.01);
        assert_eq!(out.len(), 2);
        assert!(out[0].closed && out[0].segments.len() == 1);
        assert_eq!(out[0].orientation(), Some(Orientation::Ccw));

        // middle segment first: chain must grow at both ends
        let out = chain_segments(
            &[
                line(1.0, 0.0, 2.0, 0.0),
                line(0.0, 0.0, 1.0, 0.0),
                line(2.0, 0.0, 3.0, 0.0),
            ],
            0.01,
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].start().approx_eq(Point::new(0.0, 0.0), T));
        assert!(out[0].end().approx_eq(Point::new(3.0, 0.0), T));
    }
}
