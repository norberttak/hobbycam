use crate::{Point, EPS};
use serde::{Deserialize, Serialize};

/// Axis aligned bounding box. Empty when min > max.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub min: Point,
    pub max: Point,
}

impl BBox {
    pub fn empty() -> BBox {
        BBox {
            min: Point::new(f64::INFINITY, f64::INFINITY),
            max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn include_point(&mut self, p: Point) {
        self.min = Point::new(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = Point::new(self.max.x.max(p.x), self.max.y.max(p.y));
    }

    pub fn union(&self, o: &BBox) -> BBox {
        let mut b = *self;
        if !o.is_empty() {
            b.include_point(o.min);
            b.include_point(o.max);
        }
        b
    }

    /// True if `o` lies inside self (boundaries included, EPS tolerance).
    pub fn contains_bbox(&self, o: &BBox) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && o.min.x >= self.min.x - EPS
            && o.min.y >= self.min.y - EPS
            && o.max.x <= self.max.x + EPS
            && o.max.y <= self.max.y + EPS
    }

    pub fn width(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.x - self.min.x
        }
    }

    pub fn height(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.y - self.min.y
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_union_contains() {
        let e = BBox::empty();
        assert!(e.is_empty());
        assert_eq!(e.width(), 0.0);
        let mut a = BBox::empty();
        a.include_point(Point::new(0.0, 0.0));
        a.include_point(Point::new(10.0, 5.0));
        assert!(!a.is_empty());
        assert_eq!((a.width(), a.height()), (10.0, 5.0));
        let mut b = BBox::empty();
        b.include_point(Point::new(2.0, 2.0));
        assert!(a.contains_bbox(&b));
        assert!(!b.contains_bbox(&a));
        assert!(!a.contains_bbox(&e));
        assert_eq!(a.union(&e), a);
        let u = b.union(&BBox {
            min: Point::new(-1.0, 3.0),
            max: Point::new(1.0, 4.0),
        });
        assert_eq!(u.min, Point::new(-1.0, 2.0));
        assert_eq!(u.max, Point::new(2.0, 4.0));
    }
}
