//! Canvas view transform (world mm, Y up ↔ screen px, Y down) and grid spacing.

use eframe::egui::{Pos2, Rect, Vec2};
use hc_geom::{BBox, Point};
use hc_project::Units;

pub const MIN_ZOOM: f32 = 0.001;
pub const MAX_ZOOM: f32 = 10_000.0;
pub const FIT_MARGIN_PX: f64 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// px per mm
    pub zoom: f32,
    /// Screen position of world (0,0) relative to the canvas rect min.
    pub offset: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: 1.0,
            offset: Vec2::ZERO,
        }
    }
}

impl View {
    pub fn world_to_screen(&self, rect: Rect, p: Point) -> Pos2 {
        let z = self.zoom as f64;
        Pos2::new(
            (rect.min.x as f64 + self.offset.x as f64 + p.x * z) as f32,
            (rect.min.y as f64 + self.offset.y as f64 - p.y * z) as f32,
        )
    }

    pub fn screen_to_world(&self, rect: Rect, s: Pos2) -> Point {
        let z = self.zoom as f64;
        Point::new(
            (s.x as f64 - rect.min.x as f64 - self.offset.x as f64) / z,
            (rect.min.y as f64 + self.offset.y as f64 - s.y as f64) / z,
        )
    }

    /// Zoom by `factor` keeping the world point under `screen` fixed.
    pub fn zoom_at(&mut self, rect: Rect, screen: Pos2, factor: f32) {
        let w = self.screen_to_world(rect, screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let z = self.zoom as f64;
        self.offset = Vec2::new(
            (screen.x as f64 - rect.min.x as f64 - w.x * z) as f32,
            (screen.y as f64 - rect.min.y as f64 + w.y * z) as f32,
        );
    }

    /// Fit `bbox` (or `stock` when `bbox` is empty) into `rect` with a 20 px margin.
    pub fn fit(&mut self, rect: Rect, bbox: &BBox, stock: &BBox) {
        let b = if bbox.is_empty() { stock } else { bbox };
        if b.is_empty() {
            return;
        }
        let (mut bw, mut bh) = (b.width(), b.height());
        if bw < hc_geom::EPS {
            bw = bh;
        }
        if bh < hc_geom::EPS {
            bh = bw;
        }
        if bw < hc_geom::EPS {
            (bw, bh) = (1.0, 1.0);
        }
        let (w, h) = (rect.width() as f64, rect.height() as f64);
        let m = FIT_MARGIN_PX;
        let z = ((w - 2.0 * m) / bw).min((h - 2.0 * m) / bh);
        self.zoom = (z as f32).clamp(MIN_ZOOM, MAX_ZOOM);
        let z = self.zoom as f64;
        let c = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
        self.offset = Vec2::new((w / 2.0 - c.x * z) as f32, (h / 2.0 + c.y * z) as f32);
    }

    /// World rectangle visible in `rect`.
    pub fn visible_world(&self, rect: Rect) -> BBox {
        let mut b = BBox::empty();
        b.include_point(self.screen_to_world(rect, rect.min));
        b.include_point(self.screen_to_world(rect, rect.max));
        b
    }
}

/// Minimum minor grid spacing on screen (canvas grid and grid snapping).
pub const GRID_MIN_PX: f32 = 15.0;

/// Smallest grid step (mm) of {1,2,5}·10^k display units that is at least `min_px` on screen.
pub fn grid_step(zoom: f32, units: Units, min_px: f32) -> f64 {
    let unit_mm = hc_project::from_display(1.0, units);
    let min_display = min_px as f64 / zoom as f64 / unit_mm;
    let mut k = min_display.log10().floor() as i32 - 1;
    loop {
        for m in [1.0, 2.0, 5.0] {
            let step = m * 10f64.powi(k);
            if step >= min_display * (1.0 - 1e-9) {
                return step * unit_mm;
            }
        }
        k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    fn bb(x0: f64, y0: f64, x1: f64, y1: f64) -> BBox {
        BBox {
            min: Point::new(x0, y0),
            max: Point::new(x1, y1),
        }
    }

    #[test]
    fn t1_round_trip_and_y_up() {
        let v = View {
            zoom: 3.7,
            offset: Vec2::new(10.0, 20.0),
        };
        let w = Point::new(123.4, -56.7);
        let back = v.screen_to_world(rect(), v.world_to_screen(rect(), w));
        assert!(back.distance(w) < 1e-4, "{back:?}");
        let o = v.world_to_screen(rect(), Point::new(0.0, 0.0));
        assert!((o - Pos2::new(10.0, 20.0)).length() < 1e-3);
        let y1 = v.world_to_screen(rect(), Point::new(0.0, 1.0));
        assert!((y1.y - 16.3).abs() < 1e-3, "{y1:?}");
    }

    #[test]
    fn t2_zoom_at_keeps_point_and_clamps() {
        let mut v = View {
            zoom: 3.7,
            offset: Vec2::new(10.0, 20.0),
        };
        let s = Pos2::new(400.0, 300.0);
        let before = v.screen_to_world(rect(), s);
        v.zoom_at(rect(), s, 1.25);
        assert!(v.screen_to_world(rect(), s).distance(before) < 1e-4);
        assert!((v.zoom - 3.7 * 1.25).abs() < 1e-4);
        let mut v = View {
            zoom: 9000.0,
            offset: Vec2::ZERO,
        };
        v.zoom_at(rect(), s, 2.0);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn t3_fit() {
        let stock = bb(0.0, 0.0, 100.0, 100.0);
        let mut v = View::default();
        v.fit(rect(), &bb(-12.7, -12.7, 457.2, 190.5), &stock);
        assert!(close(v.zoom as f64, 960.0 / 469.9, 1e-4), "{}", v.zoom);
        assert!(close(v.zoom as f64, 2.04299, 1e-4));
        let c = v.world_to_screen(rect(), Point::new(222.25, 88.9));
        assert!((c - Pos2::new(500.0, 300.0)).length() < 1e-3, "{c:?}");

        let mut v = View::default();
        v.fit(rect(), &BBox::empty(), &stock);
        assert!(close(v.zoom as f64, 5.6, 1e-4));
        let c = v.world_to_screen(rect(), Point::new(50.0, 50.0));
        assert!((c - Pos2::new(500.0, 300.0)).length() < 1e-3);
    }

    #[test]
    fn fit_degenerate_bbox() {
        let stock = bb(0.0, 0.0, 100.0, 100.0);
        let mut v = View::default();
        // horizontal line 100 long: height uses the width
        v.fit(rect(), &bb(0.0, 5.0, 100.0, 5.0), &stock);
        assert!(close(v.zoom as f64, 5.6, 1e-4));
        // single point: 1 mm
        v.fit(rect(), &bb(3.0, 3.0, 3.0, 3.0), &stock);
        assert!(close(v.zoom as f64, 560.0, 1e-3));
    }

    #[test]
    fn t4_grid_step() {
        let mm = |z| grid_step(z, Units::Mm, 15.0);
        for (z, want) in [
            (1.0, 20.0),
            (3.0, 5.0),
            (10.0, 2.0),
            (0.1, 200.0),
            (100.0, 0.2),
        ] {
            assert!(close(mm(z), want, 1e-9), "zoom {z}: {}", mm(z));
        }
        assert!(close(grid_step(1.0, Units::Inch, 15.0), 25.4, 1e-9));
        assert!(close(grid_step(10.0, Units::Inch, 15.0), 2.54, 1e-9));
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))
    }

    fn bb(x0: f64, y0: f64, x1: f64, y1: f64) -> BBox {
        BBox {
            min: Point::new(x0, y0),
            max: Point::new(x1, y1),
        }
    }

    fn finite(v: &View) -> bool {
        v.zoom.is_finite() && v.offset.x.is_finite() && v.offset.y.is_finite()
    }

    #[test]
    fn zoom_clamps_min_and_max_keep_point_fixed() {
        let s = Pos2::new(123.0, 456.0);
        let mut v = View {
            zoom: 0.002,
            offset: Vec2::new(7.0, 9.0),
        };
        let w = v.screen_to_world(rect(), s);
        v.zoom_at(rect(), s, 0.1);
        assert_eq!(v.zoom, MIN_ZOOM);
        assert!(v.screen_to_world(rect(), s).distance(w) < 1e-2);
        v.zoom_at(rect(), s, 0.5); // already at min
        assert_eq!(v.zoom, MIN_ZOOM);
        let mut v = View {
            zoom: MAX_ZOOM,
            offset: Vec2::ZERO,
        };
        let w = v.screen_to_world(rect(), s);
        v.zoom_at(rect(), s, 1.2);
        assert_eq!(v.zoom, MAX_ZOOM);
        assert!(v.screen_to_world(rect(), s).distance(w) < 1e-6);
        // degenerate factors never produce zoom 0 or a negative zoom
        for f in [0.0, -1.0] {
            let mut v = View::default();
            v.zoom_at(rect(), s, f);
            assert_eq!(v.zoom, MIN_ZOOM, "factor {f}");
            assert!(finite(&v));
        }
    }

    #[test]
    fn zoom_in_out_returns_to_same_view() {
        let mut v = View {
            zoom: 2.0,
            offset: Vec2::new(30.0, 500.0),
        };
        let orig = v;
        let s = Pos2::new(640.0, 100.0);
        for _ in 0..10 {
            v.zoom_at(rect(), s, 1.2);
        }
        for _ in 0..10 {
            v.zoom_at(rect(), s, 1.0 / 1.2);
        }
        assert!((v.zoom - orig.zoom).abs() < 1e-4);
        assert!((v.offset - orig.offset).length() < 1e-2);
    }

    #[test]
    fn zero_size_and_tiny_canvas_fit_stays_finite() {
        let stock = bb(0.0, 0.0, 100.0, 100.0);
        for r in [
            Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::ZERO),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(30.0, 30.0)), // smaller than the margins
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 0.0)),
        ] {
            let mut v = View::default();
            v.fit(r, &bb(-12.7, -12.7, 457.2, 190.5), &stock);
            assert!(finite(&v), "{r:?} {v:?}");
            assert!((MIN_ZOOM..=MAX_ZOOM).contains(&v.zoom));
            let back = v.screen_to_world(r, v.world_to_screen(r, Point::new(1.0, 2.0)));
            assert!(back.x.is_finite() && back.y.is_finite());
        }
    }

    #[test]
    fn fit_both_empty_leaves_view_unchanged() {
        let mut v = View {
            zoom: 3.0,
            offset: Vec2::new(1.0, 2.0),
        };
        v.fit(rect(), &BBox::empty(), &BBox::empty());
        assert_eq!(
            v,
            View {
                zoom: 3.0,
                offset: Vec2::new(1.0, 2.0)
            }
        );
    }

    #[test]
    fn very_large_world_1e5_mm() {
        let stock = bb(0.0, 0.0, 100.0, 100.0);
        let mut v = View::default();
        v.fit(rect(), &bb(0.0, 0.0, 1e5, 1e5), &stock);
        assert!((v.zoom as f64 - 560.0 / 1e5).abs() < 1e-7);
        let c = v.world_to_screen(rect(), Point::new(5e4, 5e4));
        assert!((c - Pos2::new(500.0, 300.0)).length() < 1e-2, "{c:?}");
        // zoomed far in at a point 1e5 mm from the origin: the point under the cursor stays put
        let far = Point::new(1e5, -1e5);
        let mut v = View {
            zoom: 10.0,
            offset: Vec2::ZERO,
        };
        let s = v.world_to_screen(rect(), far);
        v.offset += Pos2::new(500.0, 300.0) - s; // bring `far` to the centre
        let s = Pos2::new(500.0, 300.0);
        assert!(v.screen_to_world(rect(), s).distance(far) < 1e-2);
        for _ in 0..20 {
            v.zoom_at(rect(), s, 1.5);
        }
        assert_eq!(v.zoom, MAX_ZOOM);
        let w = v.screen_to_world(rect(), s);
        // spec stores offset as f32: at 1e9 px one ulp is 64 px = 0.0064 mm at max zoom, and
        // each zoom step re-quantises; 20 steps stay within a few ulps
        assert!(w.distance(far) < 0.05, "{w:?}");
    }

    #[test]
    fn y_flip_and_rect_origin() {
        let r = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(400.0, 300.0));
        let v = View {
            zoom: 2.0,
            offset: Vec2::new(10.0, 290.0),
        };
        let o = v.world_to_screen(r, Point::new(0.0, 0.0));
        assert_eq!(o, Pos2::new(110.0, 340.0));
        let up = v.world_to_screen(r, Point::new(0.0, 10.0));
        let right = v.world_to_screen(r, Point::new(10.0, 0.0));
        assert_eq!(up, Pos2::new(110.0, 320.0)); // +Y world = up on screen
        assert_eq!(right, Pos2::new(130.0, 340.0));
        let w = v.screen_to_world(r, Pos2::new(110.0, 50.0));
        assert!((w.y - 145.0).abs() < 1e-9 && w.x.abs() < 1e-9);
        let vis = v.visible_world(r);
        assert!(vis.min.approx_eq(Point::new(-5.0, -5.0), 1e-9));
        assert!(vis.max.approx_eq(Point::new(195.0, 145.0), 1e-9));
    }

    #[test]
    fn zoom_at_cursor_outside_rect() {
        let mut v = View {
            zoom: 1.5,
            offset: Vec2::new(200.0, 200.0),
        };
        for s in [Pos2::new(-300.0, -50.0), Pos2::new(5000.0, 900.0)] {
            let w = v.screen_to_world(rect(), s);
            v.zoom_at(rect(), s, 1.2);
            assert!(v.screen_to_world(rect(), s).distance(w) < 1e-3, "{s:?}");
        }
    }

    fn mantissa_ok(step: f64) -> bool {
        let k = step.log10().floor();
        let m = step / 10f64.powf(k);
        [1.0, 2.0, 5.0, 10.0].iter().any(|x| (m - x).abs() < 1e-6)
    }

    fn prev_125(step: f64) -> f64 {
        let k = step.log10().floor();
        let m = (step / 10f64.powf(k)).round();
        let base = 10f64.powf(k);
        match m as i64 {
            1 => 0.5 * base,
            2 => base,
            _ => 2.0 * base,
        }
    }

    #[test]
    fn grid_step_1_2_5_across_zooms() {
        let mut z = MIN_ZOOM;
        while z <= MAX_ZOOM {
            for (u, unit_mm) in [(Units::Mm, 1.0), (Units::Inch, 25.4)] {
                let step = grid_step(z, u, 15.0);
                let disp = step / unit_mm;
                assert!(mantissa_ok(disp), "zoom {z} {u:?}: {disp}");
                let px = step * z as f64;
                assert!(px >= 15.0 * (1.0 - 1e-6), "zoom {z} {u:?}: {px} px");
                assert!(px < 15.0 * 2.5 + 1e-6, "zoom {z} {u:?}: {px} px");
                // the next smaller 1-2-5 step would be under min_px (smallest wins)
                assert!(prev_125(disp) * unit_mm * (z as f64) < 15.0 * (1.0 + 1e-6));
            }
            z *= 1.37;
        }
    }

    #[test]
    fn grid_step_exact_boundary_and_other_min_px() {
        // 1 mm is exactly 15 px at zoom 15
        assert_eq!(grid_step(15.0, Units::Mm, 15.0), 1.0);
        assert_eq!(grid_step(50.0, Units::Mm, 100.0), 2.0);
        // inch: 0.01 in = 0.254 mm; zoom 100 -> 25.4 px >= 15
        assert!((grid_step(100.0, Units::Inch, 15.0) - 0.254).abs() < 1e-12);
        // 15 px / 0.01 px/mm = 1500 mm = 59.06 in -> 100 in
        assert!((grid_step(0.01, Units::Inch, 15.0) - 100.0 * 25.4).abs() < 1e-9);
    }
}
