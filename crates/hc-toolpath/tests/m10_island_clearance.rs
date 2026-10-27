//! M10 verifier, round 2: the ISLAND_CLEARANCE fix in `offset::pocket_rings` (architect review fix 1).
//! Every cutting feed is sampled every 0.02 mm; the tool edge may enter the island by at most the clearance
//! (5e-4 mm, + 5e-4 sampling/numeric slack = OK) and must never pass the boundary wall.

use hc_geom::{shapes, Circle, Contour, Orientation, Point};
use hc_project::{Geom, OpKind, Project, Tool, ToolKind, Units};
use hc_toolpath::{generate_op, Move, ToolpathError};

const OK: f64 = 1e-3;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn poly(v: &[(f64, f64)]) -> Contour {
    let v: Vec<(Point, f64)> = v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
    Contour::from_bulge_vertices(&v, true)
}

fn circle(x: f64, y: f64, r: f64) -> Contour {
    Circle {
        center: p(x, y),
        radius: r,
    }
    .to_contour()
}

fn tool(d: f64) -> Tool {
    Tool {
        id: 1,
        name: "Flat".into(),
        kind: ToolKind::FlatEndMill,
        diameter: d,
        flute_length: 12.0,
        flutes: 2,
        feed: 600.0,
        plunge_feed: 150.0,
        rpm: 16000,
        depth_per_pass: 2.0,
        stepover_pct: 45.0,
        comment: String::new(),
    }
}

/// Signed depth of `q` inside closed `c` (> 0 inside).
fn depth(c: &Contour, q: Point) -> f64 {
    let d = c.distance_to(q);
    if c.contains_point(q) {
        d
    } else {
        -d
    }
}

struct Case {
    boundary: Contour,
    islands: Vec<Contour>,
    d: f64,
    units: Units,
}

impl Case {
    fn new(boundary: Contour, islands: Vec<Contour>, d: f64) -> Case {
        Case {
            boundary,
            islands,
            d,
            units: Units::Mm,
        }
    }

    fn project(&self) -> Project {
        let mut pr = Project::new();
        pr.units = self.units;
        let b = self.boundary.bbox();
        pr.stock.min_x = b.min.x - 20.0;
        pr.stock.min_y = b.min.y - 20.0;
        pr.stock.width = b.width() + 40.0;
        pr.stock.height = b.height() + 40.0;
        let mut g = vec![Geom::Contour(self.boundary.clone())];
        g.extend(self.islands.iter().cloned().map(Geom::Contour));
        let ids = pr.add_drawn(g);
        pr.add_operation(
            "Pocket",
            OpKind::Pocket { stepover_pct: 45.0 },
            &tool(self.d),
            ids,
        );
        pr.operations[0].depth = 1.0;
        pr
    }

    /// (worst intrusion of the tool edge into an island or past the wall, where, number of cutting samples).
    fn gouge(&self) -> Result<(f64, Point, usize), ToolpathError> {
        let pr = self.project();
        let tp = generate_op(&pr, &pr.operations[0])?;
        let r = self.d / 2.0;
        let forbidden = |q: Point| {
            self.islands
                .iter()
                .map(|i| depth(i, q))
                .fold(-depth(&self.boundary, q), f64::max)
        };
        let mut worst = (f64::NEG_INFINITY, p(0.0, 0.0));
        let mut n = 0usize;
        let mut probe = |q: Point, z: f64| {
            if z < -1e-6 {
                n += 1;
                let g = forbidden(q) + r;
                if g > worst.0 {
                    worst = (g, q);
                }
            }
        };
        let mut cur = p(0.0, 0.0);
        for m in &tp.moves {
            match *m {
                Move::Rapid(to) => cur = to.xy(),
                Move::Linear { to, .. } => {
                    let k = (cur.distance(to.xy()) / 0.02).ceil().max(1.0) as usize;
                    for i in 0..=k {
                        let t = i as f64 / k as f64;
                        probe(
                            p(cur.x + (to.x - cur.x) * t, cur.y + (to.y - cur.y) * t),
                            to.z,
                        );
                    }
                    cur = to.xy();
                }
                Move::Arc {
                    to, center, ccw, ..
                } => {
                    let a0 = (cur.y - center.y).atan2(cur.x - center.x);
                    let mut a1 = (to.y - center.y).atan2(to.x - center.x);
                    if ccw && a1 <= a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    if !ccw && a1 >= a0 {
                        a1 -= std::f64::consts::TAU;
                    }
                    let rad = center.distance(cur);
                    let k = ((a1 - a0).abs() * rad / 0.02).ceil().max(1.0) as usize;
                    for i in 0..=k {
                        let a = a0 + (a1 - a0) * i as f64 / k as f64;
                        probe(p(center.x + rad * a.cos(), center.y + rad * a.sin()), to.z);
                    }
                    cur = to.xy();
                }
                Move::Drill { .. } => {}
            }
        }
        Ok((worst.0, worst.1, n))
    }

    #[track_caller]
    fn assert_clean(&self) {
        let (g, at, n) = self.gouge().expect("pocket generates");
        assert!(n > 0, "no cutting moves");
        assert!(
            g <= OK,
            "tool edge {g} mm into an island / past the wall at {at:?}"
        );
    }
}

fn sq40() -> Contour {
    shapes::rect(p(0.0, 0.0), 40.0, 40.0, 0.0).unwrap()
}

// ---------------------------------------------------------------- sharp vertices

#[test]
fn acute_triangle_tip_touching_a_wall() {
    // ~11° tip at (0,20)
    Case::new(
        sq40(),
        vec![poly(&[(0.0, 20.0), (20.0, 18.0), (20.0, 22.0)])],
        3.175,
    )
    .assert_clean();
}

#[test]
fn acute_triangle_tip_into_a_concave_boundary_corner() {
    // L-shaped boundary, concave corner at (20,20); tip exactly there
    let boundary = poly(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 20.0),
        (20.0, 20.0),
        (20.0, 40.0),
        (0.0, 40.0),
    ]);
    let tip = poly(&[(20.0, 20.0), (8.0, 10.0), (10.0, 8.0)]);
    Case::new(boundary, vec![tip], 3.175).assert_clean();
}

#[test]
fn triangle_tip_in_a_convex_boundary_corner() {
    Case::new(
        sq40(),
        vec![poly(&[(0.0, 0.0), (15.0, 5.0), (5.0, 15.0)])],
        3.175,
    )
    .assert_clean();
}

// ---------------------------------------------------------------- arc walls

#[test]
fn rect_island_touching_a_circular_wall_from_inside() {
    // boundary Ø50 at (25,25); island corner (25,0) on the circle bottom
    let island = poly(&[(25.0, 0.0), (35.0, 10.0), (25.0, 20.0), (15.0, 10.0)]);
    Case::new(circle(25.0, 25.0, 25.0), vec![island], 3.175).assert_clean();
}

#[test]
fn island_edge_on_the_straight_part_of_a_rounded_rect_boundary() {
    let b = shapes::rect(p(0.0, 0.0), 40.0, 40.0, 8.0).unwrap();
    let island = shapes::rect(p(0.0, 15.0), 10.0, 10.0, 0.0).unwrap();
    Case::new(b, vec![island], 3.175).assert_clean();
}

#[test]
fn circle_island_inside_a_rounded_corner_tangent_to_the_arc() {
    // boundary corner arc centre (8,8) r 8; island circle centre (8,8) r ... tangent from inside the pocket:
    // a Ø6 island centred on the corner bisector, touching the arc: |c - (8,8)| + 3 = 8
    let k = 5.0 / 2f64.sqrt();
    let island = circle(8.0 - k, 8.0 - k, 3.0);
    let b = shapes::rect(p(0.0, 0.0), 40.0, 40.0, 8.0).unwrap();
    Case::new(b, vec![island], 1.0).assert_clean();
}

// ---------------------------------------------------------------- L-contact, chains, island-island

#[test]
fn island_sharing_two_adjacent_edges_with_a_boundary_corner() {
    Case::new(
        sq40(),
        vec![shapes::rect(p(0.0, 0.0), 12.0, 9.0, 0.0).unwrap()],
        3.175,
    )
    .assert_clean();
}

fn chain_isl(x: f64) -> Contour {
    shapes::rect(p(x, 15.0), 6.0, 10.0, 0.0).unwrap()
}

#[test]
fn three_islands_chained_by_shared_edges() {
    // 6..12, 12..18, 18..24 share edges with each other, none with the boundary
    Case::new(
        sq40(),
        vec![chain_isl(6.0), chain_isl(12.0), chain_isl(18.0)],
        3.175,
    )
    .assert_clean();
}

#[test]
fn three_chained_islands_touching_the_wall() {
    Case::new(
        sq40(),
        vec![chain_isl(0.0), chain_isl(6.0), chain_isl(12.0)],
        3.175,
    )
    .assert_clean();
}

#[test]
fn two_islands_sharing_an_edge() {
    Case::new(sq40(), vec![chain_isl(6.0), chain_isl(12.0)], 3.175).assert_clean();
}

#[test]
fn two_islands_touching_at_one_vertex() {
    let a = shapes::rect(p(8.0, 8.0), 10.0, 10.0, 0.0).unwrap();
    let b = shapes::rect(p(18.0, 18.0), 10.0, 10.0, 0.0).unwrap();
    Case::new(sq40(), vec![a, b], 3.175).assert_clean();
}

#[test]
fn cw_drawn_island_is_shrunk_not_grown() {
    let mut i = shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap();
    i.set_orientation(Orientation::Cw);
    let mut b = sq40();
    b.set_orientation(Orientation::Cw);
    Case::new(b, vec![i], 3.175).assert_clean();
}

// ---------------------------------------------------------------- gaps around the tool diameter

#[test]
fn gap_just_below_and_above_the_tool_diameter() {
    let d = 3.175;
    for gap in [d - 1e-3, d + 1e-3, d / 2.0 - 1e-3, d / 2.0 + 1e-3] {
        let island = shapes::rect(p(gap, 10.0), 15.0, 20.0, 0.0).unwrap();
        let c = Case::new(sq40(), vec![island], d);
        let (g, at, _) = c.gouge().unwrap();
        assert!(g <= OK, "gap {gap}: {g} at {at:?}");
    }
    // two islands with a gap just below / above 2r between each other
    for gap in [d - 1e-3, d + 1e-3] {
        let a = shapes::rect(p(5.0, 10.0), 10.0, 20.0, 0.0).unwrap();
        let b = shapes::rect(p(15.0 + gap, 10.0), 10.0, 20.0, 0.0).unwrap();
        let (g, at, _) = Case::new(sq40(), vec![a, b], d).gouge().unwrap();
        assert!(g <= OK, "island gap {gap}: {g} at {at:?}");
    }
}

// ---------------------------------------------------------------- tool sizes and inch

#[test]
fn big_and_tiny_tools_on_a_shared_edge() {
    for d in [10.0, 0.5] {
        let island = shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap();
        let (g, at, _) = Case::new(sq40(), vec![island], d).gouge().unwrap();
        assert!(g <= OK, "Ø{d}: {g} at {at:?}");
        let tip = poly(&[(0.0, 20.0), (20.0, 18.0), (20.0, 22.0)]);
        let (g, at, _) = Case::new(sq40(), vec![tip], d).gouge().unwrap();
        assert!(g <= OK, "Ø{d} tip: {g} at {at:?}");
    }
}

#[test]
fn inch_sized_shared_edge() {
    let inch = 25.4;
    let b = shapes::rect(p(0.0, 0.0), 4.0 * inch, 4.0 * inch, 0.0).unwrap();
    let island = shapes::rect(p(0.0, inch), 1.5 * inch, 2.0 * inch, 0.0).unwrap();
    let mut c = Case::new(b, vec![island], 0.25 * inch);
    c.units = Units::Inch;
    c.assert_clean();
}

// ---------------------------------------------------------------- untouched islands stay byte-identical

#[test]
fn islands_farther_than_r_are_not_shrunk() {
    // the tool edge touches an unshrunk island exactly (g = 0); a shrunk one would leave g = -5e-4
    let r = 3.175 / 2.0;
    for gap in [r + 0.01, 5.0] {
        let island = shapes::rect(p(gap, 10.0), 10.0, 20.0, 0.0).unwrap();
        let (g, at, _) = Case::new(sq40(), vec![island], 3.175).gouge().unwrap();
        assert!(g.abs() < 1e-6, "gap {gap}: {g} at {at:?}");
    }
}
