//! Snap engine for the canvas tools: segment ends, contour corners, arc/circle centres, drill points.

use hc_geom::{Point, Segment, EPS};
use hc_project::{Geom, Item};

/// Snap radius in screen pixels (selection uses 6 px; snapping grabs earlier).
pub const SNAP_PX: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapKind {
    // declaration order = tie order
    End,
    Corner,
    Point,
    Center,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::End => "End",
            SnapKind::Corner => "Corner",
            SnapKind::Point => "Point",
            SnapKind::Center => "Center",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub p: Point,
    pub kind: SnapKind,
    pub item: u64,
}

/// The candidates of one item: a full circle gives only its Center; other contours give a Corner per segment start
/// (End for the ends of an open contour) and a Center per arc. Duplicates of the same kind are dropped.
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
    }
    if !c.closed {
        push(c.end(), SnapKind::End);
    }
    out
}

/// Nearest cached candidate within `radius` (world mm) among visible items; exact ties take the kind order.
pub fn nearest(
    cache: &[(usize, Point, SnapKind)],
    items: &[Item],
    visible: impl Fn(&Item) -> bool,
    p: Point,
    radius: f64,
) -> Option<Snap> {
    let mut best: Option<(f64, Snap)> = None;
    for &(idx, q, kind) in cache {
        let Some(item) = items.get(idx).filter(|i| visible(i)) else {
            continue;
        };
        let d = q.distance(p);
        if d > radius {
            continue;
        }
        let better = match best {
            None => true,
            Some((bd, b)) => d < bd - EPS || (d <= bd + EPS && kind < b.kind),
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

/// The cache for `nearest`: (item index, point, kind) of every item.
pub fn build_cache(items: &[Item]) -> Vec<(usize, Point, SnapKind)> {
    items
        .iter()
        .enumerate()
        .flat_map(|(i, it)| candidates(it).into_iter().map(move |(p, k)| (i, p, k)))
        .collect()
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
        assert_eq!((c.len(), count(&c, SnapKind::Corner)), (4, 4));
        let k = shapes::circle(p(1.0, 2.0), 3.0).unwrap();
        let c = candidates(&item(1, "0", Geom::Contour(k)));
        assert_eq!(c, [(p(1.0, 2.0), SnapKind::Center)]);
        let rr = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 2.0).unwrap();
        let c = candidates(&item(1, "0", Geom::Contour(rr)));
        assert_eq!(count(&c, SnapKind::Corner), 8);
        assert_eq!(count(&c, SnapKind::Center), 4);
        assert_eq!(c.len(), 12);
        let c = candidates(&item(1, "0", Geom::Point(p(4.0, 5.0))));
        assert_eq!(c, [(p(4.0, 5.0), SnapKind::Point)]);
        // a circle as two half arcs (bulge 1)
        let halves = Contour::from_bulge_vertices(&[(p(1.0, 0.0), 1.0), (p(-1.0, 0.0), 1.0)], true);
        let c = candidates(&item(1, "0", Geom::Contour(halves)));
        assert_eq!(count(&c, SnapKind::Corner), 2);
        assert_eq!(count(&c, SnapKind::Center), 1);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn m10_10_nearest() {
        let zoom = 2.0;
        let radius = SNAP_PX / zoom;
        let r = shapes::rect(p(0.0, 0.0), 20.0, 20.0, 0.0).unwrap();
        let items = vec![item(7, "A", Geom::Contour(r))];
        let cache = build_cache(&items);
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
        let cache = build_cache(&items);
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
            let cache = build_cache(&items);
            let s = nearest(&cache, &items, all, p(5.5, 5.0), radius).unwrap();
            assert_eq!((s.kind, s.item), (SnapKind::End, 2));
        }
    }
}
