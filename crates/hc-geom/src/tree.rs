use crate::Contour;
use serde::{Deserialize, Serialize};

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
                        && contours[j].contains_point(contours[i].start())
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
}
