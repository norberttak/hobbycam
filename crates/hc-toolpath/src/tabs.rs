//! Holding tabs: placement on the item, projection onto the cut paths, loop start rotation, lifted intervals.

use crate::{ToolpathError, ToolpathNote, POS_TOL};
use hc_geom::{Contour, Segment};
use hc_project::{TabPlacement, Tabs};

/// Lifted intervals [a, b] on one path (arc length, sorted, merged).
pub(crate) type Intervals = Vec<(f64, f64)>;

/// Tie / comparison tolerance for arc-length positions (mm).
const S_EPS: f64 = 1e-9;

/// Distance between two positions on a contour of length `l` (around the loop when closed).
fn pos_dist(a: f64, b: f64, l: f64, closed: bool) -> f64 {
    let d = (a - b).abs();
    if closed {
        let d = d.rem_euclid(l);
        d.min(l - d)
    } else {
        d
    }
}

/// Tab centres on `item` as fractions of its length (see `ManualTabs`) and an optional `TabsReduced` note.
/// A manual entry for `item_id` is returned verbatim (no note).
pub fn tab_positions(
    item: &Contour,
    tabs: &Tabs,
    item_id: u64,
    tool_diameter: f64,
) -> (Vec<f64>, Option<ToolpathNote>) {
    if let Some(m) = tabs.manual.iter().find(|m| m.item == item_id) {
        return (m.positions.clone(), None);
    }
    let l = item.length();
    let w = tabs.width;
    let d = tool_diameter;
    let requested = match tabs.placement {
        TabPlacement::Count { count } => count as usize,
        TabPlacement::Spacing { distance } => (l / distance - 1e-9).ceil().max(1.0) as usize,
    };
    let min_pitch = w + 2.0 * d;
    if requested == 0 || !min_pitch.is_finite() || min_pitch <= 0.0 || !l.is_finite() || l <= 0.0 {
        return (Vec::new(), None);
    }
    let n = requested.min((l / min_pitch).floor() as usize);

    // zones: long straight lines, minus margin m at each end
    let m = w / 2.0 + d;
    let mut zones = Vec::new();
    let mut acc = 0.0;
    for seg in &item.segments {
        let len = seg.length();
        if matches!(seg, Segment::Line { .. }) && len >= 2.0 * m {
            zones.push((acc + m, acc + len - m));
        }
        acc += len;
    }

    let dist = |a: f64, b: f64| pos_dist(a, b, l, item.closed);
    let snap = |s: f64| -> f64 {
        let mut best = (s, f64::INFINITY);
        for &(a, b) in &zones {
            let cand = if (a..=b).contains(&s) {
                s
            } else if dist(s, b) < dist(s, a) - S_EPS {
                b
            } else {
                a
            };
            if dist(s, cand) < best.1 - S_EPS {
                best = (cand, dist(s, cand));
            }
        }
        best.0
    };

    let mut kept: Vec<f64> = Vec::with_capacity(n);
    for k in 0..n {
        let s = snap(l * (k as f64 + 0.5) / n as f64);
        if kept.iter().all(|&o| dist(o, s) >= min_pitch - S_EPS) {
            kept.push(s);
        }
    }
    let note = (kept.len() < requested).then_some(ToolpathNote::TabsReduced {
        item: item_id,
        placed: kept.len(),
        requested,
    });
    (kept.into_iter().map(|s| s / l).collect(), note)
}

/// The loops of one item with their lifted intervals [a, b] (0 ≤ a < b ≤ path length, sorted, merged).
/// Each tab centre is projected onto the nearest loop; a loop whose start would lie inside a tab is
/// rotated to start in the middle of its longest free gap.
pub(crate) fn lift_paths(
    item: &Contour,
    positions: &[f64],
    loops: Vec<Contour>,
    gap: f64,
) -> Result<Vec<(Contour, Intervals)>, ToolpathError> {
    let l = item.length();
    let mut centres: Vec<Vec<f64>> = vec![Vec::new(); loops.len()];
    // shortcut: nearest-point projection may pick another wall near a narrow neck of a concave Inside path; upgrade in M8 by comparing the distance with r
    for &t in positions {
        let p = item.point_at(t.rem_euclid(1.0) * l);
        let mut best: Option<(usize, f64, f64)> = None;
        for (i, lp) in loops.iter().enumerate() {
            let (s, d) = lp.closest(p);
            if best.is_none_or(|b| d < b.2) {
                best = Some((i, s, d));
            }
        }
        if let Some((i, s, _)) = best {
            centres[i].push(s);
        }
    }
    loops
        .into_iter()
        .zip(centres)
        .map(|(lp, c)| intervals_on_loop(lp, &c, gap))
        .collect()
}

/// Merge the intervals of `centres` (each `gap` long) on a closed loop and apply the start rule.
fn intervals_on_loop(
    lp: Contour,
    centres: &[f64],
    gap: f64,
) -> Result<(Contour, Intervals), ToolpathError> {
    if centres.is_empty() {
        return Ok((lp, Vec::new()));
    }
    let p = lp.length();
    let mut iv: Vec<(f64, f64)> = centres
        .iter()
        .map(|&c| {
            let mut a = (c - gap / 2.0).rem_euclid(p);
            if a > p - POS_TOL {
                a -= p;
            }
            (a, a + gap)
        })
        .collect();
    iv.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(iv.len());
    for (a, b) in iv {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    // the last interval may wrap past the start into the first ones
    while merged.len() > 1 && merged[merged.len() - 1].1 - p >= merged[0].0 {
        let first = merged.remove(0);
        let last = merged.last_mut().expect("len > 1");
        last.1 = last.1.max(first.1 + p);
    }
    let covered: f64 = merged.iter().map(|(a, b)| b - a).sum();
    if covered >= p - POS_TOL {
        return Err(ToolpathError::InvalidParam("tabs"));
    }
    let wraps = merged.iter().any(|&(a, b)| a < -POS_TOL || b > p + POS_TOL);
    if !wraps {
        let clamped = merged
            .iter()
            .map(|&(a, b)| (a.max(0.0), b.min(p)))
            .collect();
        return Ok((lp, clamped));
    }
    // start in the middle of the longest free gap
    let n = merged.len();
    let (mid, _) = (0..n)
        .map(|i| {
            let from = merged[i].1;
            let to = if i + 1 < n {
                merged[i + 1].0
            } else {
                merged[0].0 + p
            };
            ((from + to) / 2.0, to - from)
        })
        .fold((0.0, f64::NEG_INFINITY), |best, g| {
            if g.1 > best.1 + S_EPS {
                g
            } else {
                best
            }
        });
    let mid = mid.rem_euclid(p);
    let mut shifted: Vec<(f64, f64)> = merged
        .iter()
        .map(|&(a, b)| {
            let a2 = (a - mid).rem_euclid(p);
            (a2, a2 + (b - a))
        })
        .collect();
    shifted.sort_by(|x, y| x.0.total_cmp(&y.0));
    Ok((rotated(&lp, mid), shifted))
}

/// Closed contour started at arc length `s` (the segment there is split).
pub(crate) fn rotated(c: &Contour, s: f64) -> Contour {
    let mut acc = 0.0;
    for (i, seg) in c.segments.iter().enumerate() {
        let len = seg.length();
        if s < acc + len || i + 1 == c.segments.len() {
            let local = s - acc;
            let mut segments = Vec::with_capacity(c.segments.len() + 1);
            if local < POS_TOL {
                segments.extend_from_slice(&c.segments[i..]);
                segments.extend_from_slice(&c.segments[..i]);
            } else if local > len - POS_TOL {
                segments.extend_from_slice(&c.segments[i + 1..]);
                segments.extend_from_slice(&c.segments[..=i]);
            } else {
                let (head, tail) = seg.split_at(local);
                segments.push(tail);
                segments.extend_from_slice(&c.segments[i + 1..]);
                segments.extend_from_slice(&c.segments[..i]);
                segments.push(head);
            }
            return Contour {
                segments,
                closed: c.closed,
            };
        }
        acc += len;
    }
    c.clone()
}

/// Pieces of `path` split at every interval end, each with its lifted flag. Splits within
/// `POS_TOL` of a segment end are skipped (no zero-length moves).
pub(crate) fn split_pieces(path: &Contour, iv: &[(f64, f64)]) -> Vec<(Segment, bool)> {
    let mut out = Vec::new();
    let mut s0 = 0.0;
    for seg in &path.segments {
        let len = seg.length();
        let mut cuts: Vec<f64> = iv
            .iter()
            .flat_map(|&(a, b)| [a - s0, b - s0])
            .filter(|&d| d > POS_TOL && d < len - POS_TOL)
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < POS_TOL);
        let mut rest = *seg;
        let mut done = 0.0;
        let mut ps = s0;
        let mut push = |piece: Segment, ps: &mut f64| {
            let mid = *ps + piece.length() / 2.0;
            let lifted = iv.iter().any(|&(a, b)| mid > a && mid < b);
            *ps += piece.length();
            out.push((piece, lifted));
        };
        for d in cuts {
            let (head, tail) = rest.split_at(d - done);
            push(head, &mut ps);
            rest = tail;
            done = d;
        }
        push(rest, &mut ps);
        s0 += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_geom::{Circle, Point};
    use hc_project::ManualTabs;

    fn rect() -> Contour {
        let v = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];
        Contour::from_bulge_vertices(&v.map(|(x, y)| (Point::new(x, y), 0.0)), true)
    }

    fn tabs(placement: TabPlacement) -> Tabs {
        Tabs {
            enabled: true,
            placement,
            ..Tabs::default()
        }
    }

    #[test]
    fn auto_count_and_spacing() {
        let r = rect();
        let (pos, note) = tab_positions(&r, &tabs(TabPlacement::Count { count: 4 }), 1, 6.0);
        assert_eq!(note, None);
        for (a, b) in pos.iter().zip([0.125, 0.375, 0.625, 0.875]) {
            assert!((a - b).abs() < 1e-9);
        }
        let n = |d| {
            tab_positions(&r, &tabs(TabPlacement::Spacing { distance: d }), 1, 6.0)
                .0
                .len()
        };
        assert_eq!((n(80.0), n(100.0), n(1000.0)), (4, 3, 1));
        assert!(
            tab_positions(&r, &tabs(TabPlacement::Count { count: 0 }), 1, 6.0)
                == (Vec::new(), None)
        );
    }

    #[test]
    fn manual_entry_is_verbatim() {
        let mut t = tabs(TabPlacement::Count { count: 4 });
        t.manual.push(ManualTabs {
            item: 7,
            positions: vec![0.9, 1.3],
        });
        assert_eq!(tab_positions(&rect(), &t, 7, 6.0), (vec![0.9, 1.3], None));
        assert_eq!(tab_positions(&rect(), &t, 8, 6.0).0.len(), 4);
    }

    #[test]
    fn crowded_tabs_are_dropped() {
        // a 40×2 rectangle: only the two long edges are zones, nominals snap together
        let v = [(0.0, 0.0), (40.0, 0.0), (40.0, 2.0), (0.0, 2.0)];
        let c = Contour::from_bulge_vertices(&v.map(|(x, y)| (Point::new(x, y), 0.0)), true);
        let (pos, note) = tab_positions(&c, &tabs(TabPlacement::Count { count: 4 }), 3, 1.0);
        // L 84, min_pitch 7, n_max 12 -> 4 nominals 10.5, 31.5, 52.5, 73.5; zones [3.5,36.5], [45.5,78.5]
        assert_eq!(pos.len(), 4);
        assert_eq!(note, None);
        let (pos, note) = tab_positions(&c, &tabs(TabPlacement::Count { count: 12 }), 3, 1.0);
        assert!(pos.len() < 12);
        assert_eq!(
            note,
            Some(ToolpathNote::TabsReduced {
                item: 3,
                placed: pos.len(),
                requested: 12
            })
        );
    }

    #[test]
    fn merge_and_wrap() {
        let r = rect(); // P = 300
        let (_, iv) = intervals_on_loop(r.clone(), &[50.0, 55.0, 200.0], 11.0).unwrap();
        assert_eq!(iv.len(), 2);
        assert!((iv[0].0 - 44.5).abs() < 1e-9 && (iv[0].1 - 60.5).abs() < 1e-9);
        // a tab at the start: the loop is rotated by P/2 and the interval sits in the middle
        let (lp, iv) = intervals_on_loop(r.clone(), &[0.0], 10.0).unwrap();
        assert!(lp.start().approx_eq(Point::new(100.0, 50.0), 1e-9));
        assert!((iv[0].0 - 145.0).abs() < 1e-9 && (iv[0].1 - 155.0).abs() < 1e-9);
        assert!((lp.length() - 300.0).abs() < 1e-9);
        // two tabs straddling the start merge across it
        let (_, iv) = intervals_on_loop(r.clone(), &[298.0, 5.0], 10.0).unwrap();
        assert_eq!(iv.len(), 1);
        assert!((iv[0].1 - iv[0].0 - 17.0).abs() < 1e-9);
        // full coverage is an error
        assert_eq!(
            intervals_on_loop(r, &[0.0, 100.0, 200.0], 100.0).unwrap_err(),
            ToolpathError::InvalidParam("tabs")
        );
    }

    #[test]
    fn rotated_and_split_pieces() {
        let r = rect();
        let c = rotated(&r, 30.0);
        assert!(c.start().approx_eq(Point::new(30.0, 0.0), 1e-9));
        assert!(c.end().approx_eq(Point::new(30.0, 0.0), 1e-9));
        assert_eq!(c.segments.len(), 5);
        assert_eq!(rotated(&r, 100.0).segments.len(), 4);
        let circle = Circle {
            center: Point::new(0.0, 0.0),
            radius: 10.0,
        }
        .to_contour();
        let pieces = split_pieces(&circle, &[(5.0, 15.0)]);
        assert_eq!(
            pieces.iter().map(|p| p.1).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert!((pieces[1].0.length() - 10.0).abs() < 1e-9);
        // a cut at a segment end is skipped
        let pieces = split_pieces(&r, &[(100.0, 110.0)]);
        assert_eq!(pieces.len(), 5);
        assert_eq!(pieces.iter().filter(|p| p.1).count(), 1);
    }
}
