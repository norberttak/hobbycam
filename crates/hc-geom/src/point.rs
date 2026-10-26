use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Sub};

/// 2D point, also used as a vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    pub fn scale(self, f: f64) -> Point {
        Point::new(self.x * f, self.y * f)
    }

    pub fn dot(self, o: Point) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// Z component of the 3D cross product.
    pub fn cross(self, o: Point) -> f64 {
        self.x * o.y - self.y * o.x
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, o: Point) -> f64 {
        (self - o).length()
    }

    pub fn approx_eq(self, o: Point, tol: f64) -> bool {
        self.distance(o) <= tol
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, f: f64) -> Point {
        self.scale(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_ops() {
        let a = Point::new(1.0, 2.0);
        let b = Point::new(3.0, -1.0);
        assert_eq!(a + b, Point::new(4.0, 1.0));
        assert_eq!(a - b, Point::new(-2.0, 3.0));
        assert_eq!(a * 2.0, Point::new(2.0, 4.0));
        assert!((a.dot(b) - 1.0).abs() < 1e-12);
        assert!((a.cross(b) + 7.0).abs() < 1e-12);
        assert!((Point::new(3.0, 4.0).length() - 5.0).abs() < 1e-12);
        assert!(a.approx_eq(Point::new(1.0, 2.005), 0.01));
        assert!(!a.approx_eq(Point::new(1.0, 2.05), 0.01));
    }
}
