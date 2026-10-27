//! M10 verifier: characterisation of pockets whose island touches the boundary (M10 test 15, M5 ruling e).
//! The coder's test only checks move end points; a straight feed across the island has both ends outside it,
//! so these tests sample along every feed move and compare with the tool radius (a gouge, not only the centre).

use hc_geom::{shapes, Circle, Contour, Point};
use hc_project::{Geom, OpKind, Project, Tool, ToolKind};
use hc_toolpath::{check, generate, generate_op, Move, RuleId};

const D: f64 = 3.175;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn poly(v: &[(f64, f64)]) -> Contour {
    let v: Vec<(Point, f64)> = v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
    Contour::from_bulge_vertices(&v, true)
}

fn tool() -> Tool {
    Tool {
        id: 1,
        name: "Flat".into(),
        kind: ToolKind::FlatEndMill,
        diameter: D,
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

fn pocket(boundary: Contour, island: Contour) -> Project {
    let mut pr = Project::new();
    pr.stock.min_x = -10.0;
    pr.stock.min_y = -10.0;
    pr.stock.width = 70.0;
    pr.stock.height = 70.0;
    let ids = pr.add_drawn(vec![Geom::Contour(boundary), Geom::Contour(island)]);
    pr.add_operation("Pocket", OpKind::Pocket { stepover_pct: 0.0 }, &tool(), ids);
    pr
}

/// Deepest intrusion (mm) of the tool edge into the island interior or past the boundary wall, sampled every 0.05 mm along cutting feeds.
/// `inside(q)` = distance from q into the island (> 0 inside, ≤ 0 outside).
/// Returns (intrusion, where).
fn worst_gouge(pr: &Project, depth_into: impl Fn(Point) -> f64) -> Result<(f64, Point), String> {
    let op = pr.operations[0].clone();
    let tp = generate_op(pr, &op).map_err(|e| format!("{e:?}"))?;
    let r = D / 2.0;
    let mut cur = Point::new(0.0, 0.0);
    let mut worst = (f64::NEG_INFINITY, cur);
    let mut probe = |q: Point, z: f64| {
        // the island, or the material outside the 40 x 40 boundary (every case uses that outline)
        let g = depth_into(q).max(-rect_depth(0.0, 0.0, 40.0, 40.0)(q)) + r;
        if z < -1e-6 && g > worst.0 {
            worst = (g, q);
        }
    };
    for m in &tp.moves {
        match *m {
            Move::Rapid(to) => cur = to.xy(),
            Move::Linear { to, .. } => {
                let n = (cur.distance(to.xy()) / 0.05).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    probe(
                        Point::new(cur.x + (to.x - cur.x) * t, cur.y + (to.y - cur.y) * t),
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
                let n = ((a1 - a0).abs() * rad / 0.05).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let a = a0 + (a1 - a0) * k as f64 / n as f64;
                    probe(
                        Point::new(center.x + rad * a.cos(), center.y + rad * a.sin()),
                        to.z,
                    );
                }
                cur = to.xy();
            }
            Move::Drill { .. } => {}
        }
    }
    Ok(worst)
}

/// Signed depth of `q` inside the axis-aligned rect (> 0 inside).
fn rect_depth(x0: f64, y0: f64, x1: f64, y1: f64) -> impl Fn(Point) -> f64 {
    move |q: Point| {
        let dx = (q.x - x0).min(x1 - q.x);
        let dy = (q.y - y0).min(y1 - q.y);
        if dx >= 0.0 && dy >= 0.0 {
            dx.min(dy)
        } else {
            // outside: minus the distance to the rect
            let ox = (x0 - q.x).max(q.x - x1).max(0.0);
            let oy = (y0 - q.y).max(q.y - y1).max(0.0);
            -(ox * ox + oy * oy).sqrt()
        }
    }
}

/// Signed depth of `q` inside the polygon (> 0 inside): the exact distance to its edges, sign by ray casting.
fn poly_depth(v: Vec<(f64, f64)>) -> impl Fn(Point) -> f64 {
    move |q: Point| {
        let n = v.len();
        let mut inside = false;
        let mut d = f64::INFINITY;
        for i in 0..n {
            let (a, b) = (p(v[i].0, v[i].1), p(v[(i + 1) % n].0, v[(i + 1) % n].1));
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let t = (((q.x - a.x) * ex + (q.y - a.y) * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
            d = d.min(q.distance(p(a.x + ex * t, a.y + ey * t)));
        }
        if inside {
            d
        } else {
            -d
        }
    }
}

fn shared_edge_count(pr: &Project) -> usize {
    check(pr, &generate(pr))
        .iter()
        .filter(|i| i.rule == RuleId::SharedEdge)
        .count()
}

const OK: f64 = 1e-3;

fn outer() -> Contour {
    shapes::rect(p(0.0, 0.0), 40.0, 40.0, 0.0).unwrap()
}

// ---------------------------------------------------------------- control: a gap, no contact

#[test]
fn control_island_with_a_gap_is_not_gouged() {
    let pr = pocket(
        outer(),
        shapes::rect(p(5.0, 10.0), 10.0, 20.0, 0.0).unwrap(),
    );
    let (g, at) = worst_gouge(&pr, rect_depth(5.0, 10.0, 15.0, 30.0)).unwrap();
    assert!(g <= OK, "gouge {g} at {at:?}");
    assert_eq!(shared_edge_count(&pr), 0);
}

// ---------------------------------------------------------------- A: one whole island edge on a boundary edge (spec test 15)

#[test]
fn a_whole_island_edge_on_the_boundary() {
    let pr = pocket(
        outer(),
        shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap(),
    );
    assert_eq!(shared_edge_count(&pr), 1);
    let (g, at) = worst_gouge(&pr, rect_depth(0.0, 10.0, 15.0, 30.0)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- B: partial overlap (vertices not aligned)

#[test]
fn b_island_edge_partly_overlapping_boundary_edges() {
    // the boundary's left side is split at (0,20); the island's left edge (0,10)-(0,30) overlaps half of each piece
    let boundary = poly(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 40.0),
        (0.0, 40.0),
        (0.0, 20.0),
    ]);
    let pr = pocket(
        boundary,
        shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap(),
    );
    assert_eq!(shared_edge_count(&pr), 1);
    let (g, at) = worst_gouge(&pr, rect_depth(0.0, 10.0, 15.0, 30.0)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- B2: three edges shared (island fills a full-height strip)

#[test]
fn b2_island_sharing_three_edges() {
    let pr = pocket(outer(), shapes::rect(p(0.0, 0.0), 10.0, 40.0, 0.0).unwrap());
    let (g, at) = worst_gouge(&pr, rect_depth(0.0, 0.0, 10.0, 40.0)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- C: a single vertex on a boundary edge

#[test]
fn c_island_vertex_on_a_boundary_edge() {
    // CCW diamond with its left vertex on x = 0
    let v = vec![(0.0, 20.0), (10.0, 10.0), (20.0, 20.0), (10.0, 30.0)];
    let pr = pocket(outer(), poly(&v));
    let (g, at) = worst_gouge(&pr, poly_depth(v)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

#[test]
fn c2_island_vertex_on_a_boundary_corner() {
    let v = vec![(0.0, 0.0), (10.0, 5.0), (5.0, 10.0)];
    let pr = pocket(outer(), poly(&v));
    let (g, at) = worst_gouge(&pr, poly_depth(v)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- D: circle island tangent to the boundary

#[test]
fn d_circle_island_tangent_to_the_boundary() {
    let island = Circle {
        center: p(10.0, 20.0),
        radius: 10.0,
    }
    .to_contour();
    let pr = pocket(outer(), island);
    let (g, at) = worst_gouge(&pr, |q: Point| 10.0 - q.distance(p(10.0, 20.0))).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- E: a gap narrower than the tool (no contact)

#[test]
fn e_island_with_a_gap_narrower_than_the_tool() {
    let pr = pocket(
        outer(),
        shapes::rect(p(1.0, 10.0), 14.0, 20.0, 0.0).unwrap(),
    );
    let (g, at) = worst_gouge(&pr, rect_depth(1.0, 10.0, 15.0, 30.0)).unwrap();
    assert!(g <= OK, "gouge {g} mm into the island or wall at {at:?}");
}

// ---------------------------------------------------------------- safety net: a gouging pocket must at least be warned (rule 26)

/// Gouges and SharedEdge count of a 40 x 40 pocket with `island`.
fn gouge_and_warning(island: Contour, depth: impl Fn(Point) -> f64) -> (f64, Point, usize) {
    let pr = pocket(outer(), island);
    let (g, at) = worst_gouge(&pr, depth).unwrap();
    (g, at, shared_edge_count(&pr))
}

#[test]
fn net_whole_edge_is_warned() {
    let (g, _, n) = gouge_and_warning(
        shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap(),
        rect_depth(0.0, 10.0, 15.0, 30.0),
    );
    assert!(g <= OK || n == 1, "gouge {g}, {n} warnings");
}

#[test]
fn net_near_touching_gap_is_warned() {
    for gap in [5e-5, 1e-5, 2e-6] {
        let (g, at, n) = gouge_and_warning(
            shapes::rect(p(gap, 10.0), 15.0 - gap, 20.0, 0.0).unwrap(),
            rect_depth(gap, 10.0, 15.0, 30.0),
        );
        assert!(
            g <= OK || n == 1,
            "gap {gap}: gouge {g} at {at:?}, {n} warnings"
        );
    }
}

#[test]
fn net_vertex_on_corner_is_warned() {
    let v = vec![(0.0, 0.0), (10.0, 5.0), (5.0, 10.0)];
    let (g, at, n) = gouge_and_warning(poly(&v), poly_depth(v.clone()));
    assert!(g <= OK || n == 1, "gouge {g} at {at:?}, {n} warnings");
}

#[test]
fn net_vertex_off_midpoint_is_warned() {
    // vertex at (0,12); the boundary's left edge midpoint is (0,20), so only luck made case C warn
    let v = vec![(0.0, 12.0), (10.0, 6.0), (20.0, 12.0), (10.0, 18.0)];
    let (g, at, n) = gouge_and_warning(poly(&v), poly_depth(v.clone()));
    assert!(g <= OK || n == 1, "gouge {g} at {at:?}, {n} warnings");
}

#[test]
// it gouges 7.8 mm, but rule 26 warns
fn net_tangent_circle_off_midpoint_is_warned() {
    let c = p(8.0, 12.0);
    let island = Circle {
        center: c,
        radius: 8.0,
    }
    .to_contour();
    let (g, at, n) = gouge_and_warning(island, move |q: Point| 8.0 - q.distance(c));
    assert!(g <= OK || n == 1, "gouge {g} at {at:?}, {n} warnings");
}
