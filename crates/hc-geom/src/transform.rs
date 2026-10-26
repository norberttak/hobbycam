use crate::Point;
use serde::{Deserialize, Serialize};

/// 2D affine transform: x' = a*x + c*y + tx ; y' = b*x + d*y + ty.
/// Only rigid motions, mirrors and uniform scales are meant to be used (arcs stay arcs).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Transform {
    pub fn identity() -> Transform {
        Transform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn translate(dx: f64, dy: f64) -> Transform {
        Transform {
            tx: dx,
            ty: dy,
            ..Transform::identity()
        }
    }

    /// Linear part `l` applied around the fixed point `about`.
    fn about(l: Transform, about: Point) -> Transform {
        let moved = l.apply(about);
        Transform {
            tx: about.x - moved.x,
            ty: about.y - moved.y,
            ..l
        }
    }

    /// Rotate CCW by `angle` (radians) around `about`.
    pub fn rotate(angle: f64, about: Point) -> Transform {
        let (s, c) = angle.sin_cos();
        let l = Transform {
            a: c,
            b: s,
            c: -s,
            d: c,
            ..Transform::identity()
        };
        Transform::about(l, about)
    }

    /// Uniform scale around `about`; factor must be > 0.
    pub fn scale(factor: f64, about: Point) -> Transform {
        let l = Transform {
            a: factor,
            d: factor,
            ..Transform::identity()
        };
        Transform::about(l, about)
    }

    /// Flip across the horizontal line y = about_y.
    pub fn mirror_x(about_y: f64) -> Transform {
        Transform {
            d: -1.0,
            ty: 2.0 * about_y,
            ..Transform::identity()
        }
    }

    /// Flip across the vertical line x = about_x.
    pub fn mirror_y(about_x: f64) -> Transform {
        Transform {
            a: -1.0,
            tx: 2.0 * about_x,
            ..Transform::identity()
        }
    }

    /// Apply self, then `next`.
    pub fn then(&self, next: &Transform) -> Transform {
        let (s, n) = (self, next);
        Transform {
            a: n.a * s.a + n.c * s.b,
            b: n.b * s.a + n.d * s.b,
            c: n.a * s.c + n.c * s.d,
            d: n.b * s.c + n.d * s.d,
            tx: n.a * s.tx + n.c * s.ty + n.tx,
            ty: n.b * s.tx + n.d * s.ty + n.ty,
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }

    pub(crate) fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    pub fn is_mirroring(&self) -> bool {
        self.determinant() < 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn basic_transforms() {
        let p = Point::new(1.0, 2.0);
        assert_eq!(Transform::identity().apply(p), p);
        assert!(Transform::rotate(FRAC_PI_2, Point::new(1.0, 1.0))
            .apply(Point::new(2.0, 1.0))
            .approx_eq(Point::new(1.0, 2.0), 1e-9));
        assert!(Transform::scale(2.0, Point::new(1.0, 1.0))
            .apply(Point::new(2.0, 1.0))
            .approx_eq(Point::new(3.0, 1.0), 1e-9));
        assert_eq!(Transform::mirror_x(1.0).apply(p), Point::new(1.0, 0.0));
        assert_eq!(Transform::mirror_y(1.0).apply(p), Point::new(1.0, 2.0));
        assert!(Transform::mirror_x(0.0).is_mirroring());
        assert!(!Transform::rotate(1.0, p).is_mirroring());
    }

    #[test]
    fn test_16_then_translate_then_rotate() {
        let t = Transform::translate(1.0, 0.0)
            .then(&Transform::rotate(FRAC_PI_2, Point::new(0.0, 0.0)));
        assert!(t
            .apply(Point::new(0.0, 0.0))
            .approx_eq(Point::new(0.0, 1.0), 1e-9));
    }
}
