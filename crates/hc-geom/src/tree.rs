use crate::{Contour, Point};
use serde::{Deserialize, Serialize};

/// Sample points closer than this (mm) to the parent's edge say nothing about containment.
const ON_EDGE_TOL: f64 = 1e-6;

/// A point of `child` that is not on `parent`'s edge: the start, else the first such segment midpoint.
/// Falls back to the start when every candidate is on the edge (e.g. identical contours).
fn sample_off_edge(child: &Contour, parent: &Contour) -> Point {
    std::iter::once(child.start())
        .chain(child.segments.iter().map(|s| s.midpoint()))
        .find(|&p| parent.distance_to(p) > ON_EDGE_TOL)
        .unwrap_or_else(|| child.start())
}

/// Nesting info for the contour at index `contour` of the input slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainmentNode {
    pub contour: usize,
    pub parent: Option<usize>,
    pub depth: usize,
}

/// One node per input contour (same order). Parent = smallest closed contour enclosing it.
/// Open contours are depth 0 and never parents. Assumes non-intersecting contours.
pub fn containment_tree(contours: &[Contour]) -> Vec<ContainmentNode> {
    let areas: Vec<f64> = contours.iter().map(|c| c.signed_area().abs()).collect();
    let bboxes: Vec<_> = contours.iter().map(Contour::bbox).collect();
    let usable = |c: &Contour| c.closed && !c.segments.is_empty();

    let parents: Vec<Option<usize>> = (0..contours.len())
        .map(|i| {
            if !usable(&contours[i]) {
                return None;
            }
            (0..contours.len())
                .filter(|&j| {
                    j != i
                        && usable(&contours[j])
                        && areas[j] > areas[i]
                        && bboxes[j].contains_bbox(&bboxes[i])
                        && contours[j].contains_point(sample_off_edge(&contours[i], &contours[j]))
                })
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        })
        .collect();

    (0..contours.len())
        .map(|i| {
            // parents have strictly larger area, so this walk terminates
            let mut depth = 0;
            let mut cur = parents[i];
            while let Some(p) = cur {
                depth += 1;
                cur = parents[p];
            }
            ContainmentNode {
                contour: i,
                parent: parents[i],
                depth,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contour::tests::polygon;
    use crate::{Circle, Point};

    fn square(cx: f64, cy: f64, s: f64) -> Contour {
        let h = s / 2.0;
        polygon(&[
            (cx - h, cy - h),
            (cx + h, cy - h),
            (cx + h, cy + h),
            (cx - h, cy + h),
        ])
    }

    fn circle(x: f64, y: f64, r: f64) -> Contour {
        Circle {
            center: Point::new(x, y),
            radius: r,
        }
        .to_contour()
    }

    #[test]
    fn test_18_circle_in_square() {
        let t = containment_tree(&[square(5.0, 5.0, 10.0), circle(5.0, 5.0, 2.0)]);
        assert_eq!((t[0].parent, t[0].depth), (None, 0));
        assert_eq!((t[1].parent, t[1].depth), (Some(0), 1));
        assert_eq!((t[0].contour, t[1].contour), (0, 1));
    }

    #[test]
    fn test_19_three_levels_reverse_order_plus_open() {
        let mut open = square(20.0, 20.0, 1.0);
        open.closed = false;
        open.segments.pop();
        let t = containment_tree(&[
            square(0.0, 0.0, 2.0),
            square(0.0, 0.0, 6.0),
            square(0.0, 0.0, 10.0),
            open,
        ]);
        assert_eq!((t[0].parent, t[0].depth), (Some(1), 2));
        assert_eq!((t[1].parent, t[1].depth), (Some(2), 1));
        assert_eq!((t[2].parent, t[2].depth), (None, 0));
        assert_eq!((t[3].parent, t[3].depth), (None, 0));
    }

    #[test]
    fn child_touching_parent_at_its_start() {
        // island Ø20 at (20,0) touches the Ø60 boundary at (30,0), which is also its start point
        let t = containment_tree(&[circle(0.0, 0.0, 30.0), circle(20.0, 0.0, 10.0)]);
        assert_eq!((t[1].parent, t[1].depth), (Some(0), 1));
        // a polygon child whose first vertex lies on the parent's edge
        let tri = polygon(&[(10.0, 0.0), (5.0, 3.0), (5.0, -3.0)]);
        let t = containment_tree(&[square(5.0, 0.0, 10.0), tri]);
        assert_eq!(t[1].parent, Some(0));
        // touching from outside stays a sibling
        let t = containment_tree(&[circle(0.0, 0.0, 30.0), circle(40.0, 0.0, 10.0)]);
        assert_eq!(t[1].parent, None);
    }

    #[test]
    fn test_20_sibling_circles() {
        let t = containment_tree(&[
            circle(2.0, 2.0, 1.0),
            square(5.0, 5.0, 10.0),
            circle(7.0, 7.0, 1.5),
        ]);
        assert_eq!((t[0].parent, t[0].depth), (Some(1), 1));
        assert_eq!((t[1].parent, t[1].depth), (None, 0));
        assert_eq!((t[2].parent, t[2].depth), (Some(1), 1));
    }

    #[test]
    fn m10_15_island_sharing_whole_edges() {
        let rect =
            |x0: f64, y0: f64, x1: f64, y1: f64| polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
        // one whole edge on the parent's left edge
        let t = containment_tree(&[rect(0.0, 0.0, 20.0, 20.0), rect(0.0, 5.0, 10.0, 15.0)]);
        assert_eq!((t[1].parent, t[1].depth), (Some(0), 1));
        assert_eq!((t[0].parent, t[0].depth), (None, 0));
        // three edges on the parent
        let t = containment_tree(&[rect(0.0, 0.0, 20.0, 20.0), rect(0.0, 0.0, 10.0, 20.0)]);
        assert_eq!((t[1].parent, t[1].depth), (Some(0), 1));
        // shapes::rect island sharing its left edge with a shapes::rect boundary
        let outer = crate::shapes::rect(Point::new(0.0, 0.0), 40.0, 40.0, 0.0).unwrap();
        let island = crate::shapes::rect(Point::new(0.0, 10.0), 15.0, 20.0, 0.0).unwrap();
        let t = containment_tree(&[outer, island]);
        assert_eq!((t[1].parent, t[1].depth), (Some(0), 1));
    }
}
