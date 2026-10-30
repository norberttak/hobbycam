//! Snap engine for the canvas tools: segment ends, contour corners, arc/circle centres, drill points, segment
//! midpoints and intersections between segments; plus the grid (`grid_snap`, never in the cache).

use eframe::egui::{self, Color32, Painter, Pos2, Stroke, Vec2};
use hc_geom::{Point, Segment, DEFAULT_CHAIN_TOL, EPS};
use hc_project::{Geom, Item};
use std::f64::consts::TAU;

/// Snap radius in screen pixels (selection uses 6 px; snapping grabs earlier).
pub const SNAP_PX: f64 = 10.0;
/// More contour segments than this in the project: no intersection candidates at all.
pub const MAX_X_SEGMENTS: usize = 5000;
/// Intersection candidates stop being collected at this count.
pub const MAX_X_POINTS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapKind {
    // declaration order = tie order
    End,
    Corner,
    Point,
    Center,
    Intersection,
    Midpoint,
    Grid,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::End => "End",
            SnapKind::Corner => "Corner",
            SnapKind::Point => "Point",
            SnapKind::Center => "Center",
            SnapKind::Intersection => "Intersection",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Grid => "Grid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub p: Point,
    pub kind: SnapKind,
    pub item: u64,
}

/// Cached candidates: (item a, item b, point, kind), b == a except for intersections of two items.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapCache {
    pub cands: Vec<(usize, usize, Point, SnapKind)>,
    /// Intersections were skipped or cut off (MAX_X_SEGMENTS / MAX_X_POINTS).
    pub capped: bool,
}

fn full_circle(s: &Segment) -> bool {
    matches!(s, Segment::Arc { sweep, .. } if sweep.abs() >= TAU - EPS)
}

/// The candidates of one item: a full circle gives only its Center; other contours give a Corner per segment start
/// (End for the ends of an open contour), a Center per arc and a Midpoint per segment (not of full-circle arcs).
/// Duplicates of the same kind are dropped.
pub fn candidates(item: &Item) -> Vec<(Point, SnapKind)> {
    let c = match &item.geom {
        Geom::Point(p) => return vec![(*p, SnapKind::Point)],
        Geom::Contour(c) => c,
    };
    if c.segments.is_empty() {
        return Vec::new();
    }
    if let Some(circle) = c.as_circle() {
        return vec![(circle.center, SnapKind::Center)];
    }
    let mut out: Vec<(Point, SnapKind)> = Vec::new();
    let mut push = |p: Point, k: SnapKind| {
        if !out.iter().any(|&(q, j)| j == k && q.distance(p) <= EPS) {
            out.push((p, k));
        }
    };
    for (i, s) in c.segments.iter().enumerate() {
        let kind = if i == 0 && !c.closed {
            SnapKind::End
        } else {
            SnapKind::Corner
        };
        push(s.start(), kind);
        if let Segment::Arc { center, .. } = s {
            push(*center, SnapKind::Center);
        }
        if !full_circle(s) {
            push(s.midpoint(), SnapKind::Midpoint);
        }
    }
    if !c.closed {
        push(c.end(), SnapKind::End);
    }
    out
}

/// Nearest node of a square grid of `step` mm through the drawing origin (round half away from zero).
pub fn grid_snap(p: Point, step: f64) -> Point {
    Point::new((p.x / step).round() * step, (p.y / step).round() * step)
}

/// Nearest cached candidate within `radius` (world mm) whose item(s) are visible; exact ties take the kind order.
pub fn nearest(
    cache: &[(usize, usize, Point, SnapKind)],
    items: &[Item],
    visible: impl Fn(&Item) -> bool,
    p: Point,
    radius: f64,
) -> Option<Snap> {
    let shown = |i: usize| items.get(i).filter(|it| visible(it));
    let mut best: Option<(f64, Snap)> = None;
    for &(a, b, q, kind) in cache {
        let d = q.distance(p);
        if d > radius {
            continue;
        }
        let (Some(item), Some(_)) = (shown(a), shown(b)) else {
            continue;
        };
        let better = match best {
            None => true,
            Some((bd, s)) => d < bd - EPS || (d <= bd + EPS && kind < s.kind),
        };
        if better {
            best = Some((
                d,
                Snap {
                    p: q,
                    kind,
                    item: item.id,
                },
            ));
        }
    }
    best.map(|(_, s)| s)
}

/// True when `q` is within DEFAULT_CHAIN_TOL of an end of `s` (already an End/Corner); a full circle has no ends.
fn near_end(s: &Segment, q: Point) -> bool {
    !full_circle(s)
        && (q.distance(s.start()) <= DEFAULT_CHAIN_TOL || q.distance(s.end()) <= DEFAULT_CHAIN_TOL)
}

/// `candidates()` of every item plus the intersections between all contour segments (x-sorted bbox sweep).
pub fn build_cache(items: &[Item]) -> SnapCache {
    let mut cache = SnapCache {
        cands: items
            .iter()
            .enumerate()
            .flat_map(|(i, it)| candidates(it).into_iter().map(move |(p, k)| (i, i, p, k)))
            .collect(),
        capped: false,
    };
    let mut segs: Vec<(usize, &Segment, hc_geom::BBox)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match &it.geom {
            Geom::Contour(c) => Some((i, c)),
            Geom::Point(_) => None,
        })
        .flat_map(|(i, c)| c.segments.iter().map(move |s| (i, s, s.bbox())))
        .collect();
    if segs.len() > MAX_X_SEGMENTS {
        cache.capped = true;
        return cache;
    }
    segs.sort_by(|a, b| a.2.min.x.total_cmp(&b.2.min.x));
    let mut found = 0;
    for (k, &(ia, sa, ba)) in segs.iter().enumerate() {
        for &(ib, sb, bb) in &segs[k + 1..] {
            if bb.min.x > ba.max.x + EPS {
                break;
            }
            if bb.min.y > ba.max.y + EPS || bb.max.y < ba.min.y - EPS {
                continue;
            }
            for q in sa.intersections(sb) {
                if near_end(sa, q) || near_end(sb, q) {
                    continue;
                }
                if found == MAX_X_POINTS {
                    cache.capped = true;
                    return cache;
                }
                found += 1;
                cache
                    .cands
                    .push((ia.min(ib), ia.max(ib), q, SnapKind::Intersection));
            }
        }
    }
    cache
}

/// The marker of a snap kind, 1 px, in an 8 px box: square End/Corner, diamond Point, circle Center, X Intersection,
/// triangle Midpoint (apex up). Readable without colour.
pub fn marker(painter: &Painter, c: Pos2, kind: SnapKind, color: Color32) {
    let st = Stroke::new(1.0, color);
    let h = 4.0;
    let pts =
        |v: &[(f32, f32)]| -> Vec<Pos2> { v.iter().map(|&(x, y)| c + Vec2::new(x, y)).collect() };
    match kind {
        SnapKind::End | SnapKind::Corner => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, Vec2::splat(2.0 * h)),
                0.0,
                st,
                egui::StrokeKind::Middle,
            );
        }
        SnapKind::Point => {
            painter.add(egui::Shape::closed_line(
                pts(&[(0.0, -h), (h, 0.0), (0.0, h), (-h, 0.0)]),
                st,
            ));
        }
        SnapKind::Center => {
            painter.circle_stroke(c, h, st);
        }
        SnapKind::Intersection => {
            painter.line_segment([c + Vec2::new(-h, -h), c + Vec2::new(h, h)], st);
            painter.line_segment([c + Vec2::new(-h, h), c + Vec2::new(h, -h)], st);
        }
        SnapKind::Midpoint => {
            painter.add(egui::Shape::closed_line(
                pts(&[(0.0, -h), (h, h), (-h, h)]),
                st,
            ));
        }
        SnapKind::Grid => {
            painter.line_segment([c + Vec2::new(-h, 0.0), c + Vec2::new(h, 0.0)], st);
            painter.line_segment([c + Vec2::new(0.0, -h), c + Vec2::new(0.0, h)], st);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_geom::{shapes, Contour};
    use hc_project::Source;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn item(id: u64, layer: &str, geom: Geom) -> Item {
        Item {
            id,
            layer: layer.into(),
            source: Source::Drawn,
            geom,
        }
    }

    #[test]
    fn m20_g1_grid_snap_exact() {
        let cases = [
            ((12.3, -7.6, 5.0), (10.0, -10.0)),
            ((29.0, -31.0, 20.0), (20.0, -40.0)),
            ((3.1, 4.9, 2.0), (4.0, 4.0)),
            ((2.5, -2.5, 5.0), (5.0, -5.0)),
            ((0.04, -0.04, 0.2), (0.0, 0.0)),
            ((40.0, 60.0, 20.0), (40.0, 60.0)),
        ];
        for ((x, y, st), (ex, ey)) in cases {
            assert_eq!(grid_snap(p(x, y), st), p(ex, ey), "{x} {y} {st}");
        }
    }

    #[test]
    fn m20_g2_grid_snap_with_view_step() {
        use crate::view::{grid_step, GRID_MIN_PX};
        use hc_project::Units::{Inch, Mm};
        let cases = [
            (Mm, 1.0, (29.0, -31.0), (20.0, -40.0)),
            (Mm, 3.0, (12.3, -7.6), (10.0, -10.0)),
            (Mm, 10.0, (3.1, 4.9), (4.0, 4.0)),
            (Mm, 100.0, (1.23, -0.35), (1.2, -0.4)),
            (Inch, 1.0, (40.0, 10.0), (50.8, 0.0)),
            (Inch, 10.0, (3.9, -1.2), (5.08, 0.0)),
            (Inch, 100.0, (1.0, 0.3), (1.016, 0.254)),
        ];
        for (u, z, (x, y), (ex, ey)) in cases {
            let g = grid_snap(p(x, y), grid_step(z, u, GRID_MIN_PX));
            assert!(g.distance(p(ex, ey)) < 1e-9, "{u:?} {z}: {g:?}");
        }
    }

    #[test]
    fn m20_g3_matches_drawn_lines() {
        use crate::view::{grid_step, GRID_MIN_PX};
        for u in [hc_project::Units::Mm, hc_project::Units::Inch] {
            for z in [0.37, 1.0, 2.6, 13.0, 77.0] {
                let step = grid_step(z, u, GRID_MIN_PX);
                for q in [p(123.456, -78.9), p(-0.3, 0.7)] {
                    let g = grid_snap(q, step);
                    assert_eq!(g.x, (q.x / step).round() as i64 as f64 * step);
                    assert_eq!(g.y, (q.y / step).round() as i64 as f64 * step);
                }
            }
        }
    }

    #[test]
    fn m20_g4_grid_kind() {
        assert_eq!(SnapKind::Grid.label(), "Grid");
        assert!(SnapKind::Grid > SnapKind::Midpoint);
    }

    fn count(v: &[(Point, SnapKind)], k: SnapKind) -> usize {
        v.iter().filter(|(_, j)| *j == k).count()
    }

    #[test]
    fn m10_09_candidates() {
        let l = Contour::from_bulge_vertices(
            &[
                (p(0.0, 0.0), 0.0),
                (p(10.0, 0.0), 0.0),
                (p(10.0, 10.0), 0.0),
            ],
            false,
        );
        let c = candidates(&item(1, "0", Geom::Contour(l)));
        // M10a.2: midpoints are checked in m10a2_06
        let c: Vec<_> = c
            .into_iter()
            .filter(|x| x.1 != SnapKind::Midpoint)
            .collect();
        assert_eq!(
            c,
            [
                (p(0.0, 0.0), SnapKind::End),
                (p(10.0, 0.0), SnapKind::Corner),
                (p(10.0, 10.0), SnapKind::End)
            ]
        );
        let r = shapes::rect(p(0.0, 0.0), 10.0, 5.0, 0.0).unwrap();
        let c = candidates(&item(1, "0", Geom::Contour(r)));
        assert_eq!((c.len(), count(&c, SnapKind::Corner)), (8, 4)); // M10a.2: + 4 Midpoints
        let k = shapes::circle(p(1.0, 2.0), 3.0).unwrap();
        let c = candidates(&item(1, "0", Geom::Contour(k)));
        assert_eq!(c, [(p(1.0, 2.0), SnapKind::Center)]);
        let rr = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 2.0).unwrap();
        let c = candidates(&item(1, "0", Geom::Contour(rr)));
        assert_eq!(count(&c, SnapKind::Corner), 8);
        assert_eq!(count(&c, SnapKind::Center), 4);
        assert_eq!(c.len(), 20); // M10a.2: + 8 Midpoints
        let c = candidates(&item(1, "0", Geom::Point(p(4.0, 5.0))));
        assert_eq!(c, [(p(4.0, 5.0), SnapKind::Point)]);
        // a circle as two half arcs (bulge 1)
        let halves = Contour::from_bulge_vertices(&[(p(1.0, 0.0), 1.0), (p(-1.0, 0.0), 1.0)], true);
        let c = candidates(&item(1, "0", Geom::Contour(halves)));
        assert_eq!(count(&c, SnapKind::Corner), 2);
        assert_eq!(count(&c, SnapKind::Center), 1);
        assert_eq!(c.len(), 5); // M10a.2: + 2 Midpoints
    }

    #[test]
    fn m10_10_nearest() {
        let zoom = 2.0;
        let radius = SNAP_PX / zoom;
        // M10a.2: 40 x 40 so the edge midpoints (20, 0) stay out of reach
        let r = shapes::rect(p(0.0, 0.0), 40.0, 40.0, 0.0).unwrap();
        let items = vec![item(7, "A", Geom::Contour(r))];
        let cache = build_cache(&items).cands;
        let all = |_: &Item| true;
        let s = nearest(&cache, &items, all, p(1.5, 0.0), radius).unwrap();
        assert_eq!(s.p, p(0.0, 0.0)); // exactly the candidate
        assert_eq!((s.kind, s.item), (SnapKind::Corner, 7));
        assert_eq!(nearest(&cache, &items, all, p(5.5, 0.0), radius), None);
        assert_eq!(
            nearest(&cache, &items, |i| i.layer != "A", p(1.5, 0.0), radius),
            None
        );

        // candidates at 4 px and 6 px: the 4 px one
        let items = vec![
            item(1, "0", Geom::Point(p(3.0, 0.0))),
            item(2, "0", Geom::Point(p(-2.0, 0.0))),
        ];
        let cache = build_cache(&items).cands;
        let s = nearest(&cache, &items, all, p(0.0, 0.0), radius).unwrap();
        assert_eq!((s.p, s.item), (p(-2.0, 0.0), 2));

        // an End coincident with a Center: End wins, whatever the order
        let line = shapes::line(p(5.0, 5.0), p(9.0, 5.0)).unwrap();
        let circ = shapes::circle(p(5.0, 5.0), 1.0).unwrap();
        for items in [
            vec![
                item(1, "0", Geom::Contour(circ.clone())),
                item(2, "0", Geom::Contour(line.clone())),
            ],
            vec![
                item(2, "0", Geom::Contour(line)),
                item(1, "0", Geom::Contour(circ)),
            ],
        ] {
            let cache = build_cache(&items).cands;
            let s = nearest(&cache, &items, all, p(5.5, 5.0), radius).unwrap();
            assert_eq!((s.kind, s.item), (SnapKind::End, 2));
        }
    }

    // ------------------------------------------------------------ M10a.2

    fn seg_line(id: u64, layer: &str, a: (f64, f64), b: (f64, f64)) -> Item {
        item(
            id,
            layer,
            Geom::Contour(shapes::line(p(a.0, a.1), p(b.0, b.1)).unwrap()),
        )
    }

    fn of_kind(c: &SnapCache, k: SnapKind) -> Vec<(usize, usize, Point, SnapKind)> {
        c.cands.iter().copied().filter(|x| x.3 == k).collect()
    }

    #[test]
    fn m10a2_06_midpoints() {
        let mids = |it: &Item| -> Vec<Point> {
            candidates(it)
                .into_iter()
                .filter(|x| x.1 == SnapKind::Midpoint)
                .map(|x| x.0)
                .collect()
        };
        assert_eq!(
            mids(&seg_line(1, "0", (0.0, 0.0), (10.0, 0.0))),
            [p(5.0, 0.0)]
        );
        let r = item(
            1,
            "0",
            Geom::Contour(shapes::rect(p(0.0, 0.0), 10.0, 4.0, 0.0).unwrap()),
        );
        let m = mids(&r);
        assert_eq!(m.len(), 4);
        for want in [p(5.0, 0.0), p(10.0, 2.0), p(5.0, 4.0), p(0.0, 2.0)] {
            assert!(
                m.iter().any(|q| q.approx_eq(want, 1e-9)),
                "{want:?} in {m:?}"
            );
        }
        let quarter = Contour {
            segments: vec![Segment::Arc {
                center: p(0.0, 0.0),
                radius: 10.0,
                start_angle: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            }],
            closed: false,
        };
        let m = mids(&item(1, "0", Geom::Contour(quarter)));
        let h = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(m.len(), 1);
        assert!(m[0].approx_eq(p(h, h), 1e-9));
        let circ = item(
            1,
            "0",
            Geom::Contour(shapes::circle(p(0.0, 0.0), 3.0).unwrap()),
        );
        assert!(mids(&circ).is_empty());
    }

    #[test]
    fn m10a2_07_intersections() {
        let items = vec![
            seg_line(1, "0", (0.0, 0.0), (10.0, 10.0)),
            seg_line(2, "0", (0.0, 10.0), (10.0, 0.0)),
        ];
        let x = of_kind(&build_cache(&items), SnapKind::Intersection);
        assert_eq!(x.len(), 1);
        assert_eq!((x[0].0, x[0].1), (0, 1));
        assert!(x[0].2.approx_eq(p(5.0, 5.0), 1e-9));

        let open = |v: &[(f64, f64)]| {
            let v: Vec<(Point, f64)> = v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
            item(
                1,
                "0",
                Geom::Contour(Contour::from_bulge_vertices(&v, false)),
            )
        };
        let l = open(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        assert!(of_kind(&build_cache(&[l]), SnapKind::Intersection).is_empty());
        let bow = open(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]);
        let x = of_kind(&build_cache(&[bow]), SnapKind::Intersection);
        assert_eq!(x.len(), 1);
        assert_eq!((x[0].0, x[0].1), (0, 0));
        assert!(x[0].2.approx_eq(p(5.0, 5.0), 1e-9));

        // T: a line ending on another line's middle gives only the End
        let items = vec![
            seg_line(1, "0", (0.0, 0.0), (10.0, 0.0)),
            seg_line(2, "0", (5.0, 0.0), (5.0, 5.0)),
        ];
        let c = build_cache(&items);
        assert!(of_kind(&c, SnapKind::Intersection).is_empty());
        let s = nearest(&c.cands, &items, |_| true, p(5.0, 0.1), 1.0).unwrap();
        assert_eq!((s.kind, s.p), (SnapKind::End, p(5.0, 0.0)));
    }

    #[test]
    fn m10a2_08_intersection_needs_both_items_visible() {
        let items = vec![
            seg_line(1, "A", (0.0, 0.0), (10.0, 10.0)),
            seg_line(2, "B", (0.0, 10.0), (10.0, 0.0)),
        ];
        let c = build_cache(&items);
        let all = nearest(&c.cands, &items, |_| true, p(5.0, 5.0), 1.0).unwrap();
        assert_eq!(all.kind, SnapKind::Intersection);
        let s = nearest(&c.cands, &items, |i| i.layer != "B", p(5.0, 5.0), 1.0);
        // the crossing is A's midpoint
        assert_eq!(s.map(|s| (s.kind, s.item)), Some((SnapKind::Midpoint, 1)));
        let far = nearest(&c.cands, &items, |i| i.layer != "B", p(5.0, 5.0), 1e-3);
        assert_ne!(far.map(|s| s.kind), Some(SnapKind::Intersection));
    }

    #[test]
    fn m10a2_09_tie_order() {
        let items = vec![
            seg_line(1, "0", (0.0, 5.0), (10.0, 5.0)),
            seg_line(2, "0", (5.0, 0.0), (5.0, 10.0)),
        ];
        let c = build_cache(&items);
        let s = nearest(&c.cands, &items, |_| true, p(5.0, 5.0), 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Intersection);

        let mut items = vec![
            seg_line(1, "0", (0.0, 0.0), (10.0, 10.0)),
            seg_line(2, "0", (0.0, 10.0), (10.0, 0.0)),
        ];
        items.push(item(
            3,
            "0",
            Geom::Contour(shapes::circle(p(5.0, 5.0), 2.0).unwrap()),
        ));
        let c = build_cache(&items);
        let s = nearest(&c.cands, &items, |_| true, p(5.2, 5.0), 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Center);

        items[2] = seg_line(3, "0", (5.0, 5.0), (5.0, 9.0));
        let c = build_cache(&items);
        let s = nearest(&c.cands, &items, |_| true, p(5.2, 5.0), 1.0).unwrap();
        assert_eq!((s.kind, s.item), (SnapKind::End, 3));
    }

    #[test]
    fn m10a2_10_caps() {
        let items: Vec<Item> = (0..MAX_X_SEGMENTS as u64 + 1)
            .map(|k| seg_line(k, "0", (k as f64 * 2.0, 0.0), (k as f64 * 2.0 + 1.0, 0.0)))
            .collect();
        let c = build_cache(&items);
        assert!(c.capped);
        assert!(of_kind(&c, SnapKind::Intersection).is_empty());
        assert_eq!(of_kind(&c, SnapKind::End).len(), 2 * items.len());

        let mut grid = Vec::new();
        for k in 0..150 {
            let v = k as f64;
            grid.push(seg_line(2 * k, "0", (-1.0, v), (150.0, v)));
            grid.push(seg_line(2 * k + 1, "0", (v, -1.0), (v, 150.0)));
        }
        let c = build_cache(&grid);
        assert!(c.capped);
        assert_eq!(of_kind(&c, SnapKind::Intersection).len(), MAX_X_POINTS);
    }

    #[test]
    fn m10a2_11_performance() {
        let imp = hc_dxf::import_dxf_file(
            &crate::state::tests::bracket_path(),
            &hc_dxf::ImportOptions::default(),
        )
        .unwrap();
        let mut pr = hc_project::Project::new();
        pr.add_dxf(&imp, "b.dxf");
        let t = std::time::Instant::now();
        let c = build_cache(&pr.items);
        assert!(t.elapsed().as_millis() < 50, "{:?}", t.elapsed());
        assert!(!c.capped);

        // 2500 random lines from a seeded LCG
        let mut seed: u64 = 12345;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 200.0
        };
        let items: Vec<Item> = (0..2500)
            .map(|k| {
                let (x, y) = (rnd(), rnd());
                let (dx, dy) = (rnd() / 10.0 - 10.0, rnd() / 10.0 - 10.0);
                seg_line(k, "0", (x, y), (x + dx + 0.5, y + dy))
            })
            .collect();
        let t = std::time::Instant::now();
        let c = build_cache(&items);
        assert!(t.elapsed().as_secs_f64() < 2.0, "{:?}", t.elapsed());
        assert!(!c.capped);
        assert!(!of_kind(&c, SnapKind::Intersection).is_empty());
    }
}
