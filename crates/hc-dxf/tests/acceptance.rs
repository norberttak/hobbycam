//! M2 acceptance tests (specs/M2.md), one test per item.

use dxf::entities::*;
use dxf::enums::{AcadVersion, Units};
use dxf::{Block, Drawing, LwPolylineVertex, Vector};
use hc_dxf::*;
use hc_geom::{containment_tree, Contour, Point, Segment};
use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI};
use std::path::Path;

const T: f64 = 1e-6;
const BRACKET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dxf/autocad2018_bracket_inch.dxf"
);

fn bracket() -> DxfImport {
    import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn dpt(x: f64, y: f64) -> dxf::Point {
    dxf::Point::new(x, y, 0.0)
}

/// Radius of a closed contour that is a single full-sweep arc.
fn circle_radius(c: &Contour) -> Option<f64> {
    match c.segments.as_slice() {
        [Segment::Arc { radius, sweep, .. }] if c.closed && close(sweep.abs(), 2.0 * PI, 1e-9) => {
            Some(*radius)
        }
        _ => None,
    }
}

fn new_drawing() -> Drawing {
    let mut d = Drawing::new();
    d.header.version = AcadVersion::R2018;
    d.header.default_drawing_units = Units::Millimeters;
    d
}

fn add(d: &mut Drawing, layer: &str, specific: EntityType) {
    let mut e = Entity::new(specific);
    e.common.layer = layer.to_string();
    d.add_entity(e);
}

fn import(d: &Drawing) -> DxfImport {
    let mut buf = Vec::new();
    d.save(&mut buf).unwrap();
    import_dxf_bytes(&buf, &ImportOptions::default()).unwrap()
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> EntityType {
    EntityType::Line(Line::new(dpt(x0, y0), dpt(x1, y1)))
}

fn all_lines(c: &Contour) -> bool {
    c.segments.iter().all(|s| matches!(s, Segment::Line { .. }))
}

// ---------------------------------------------------------------- Bracket

#[test]
fn t01_bracket_units_no_warnings_no_points() {
    let r = bracket();
    assert_eq!(r.unit_scale, 25.4);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(r.layers.iter().all(|l| l.points.is_empty()));
}

#[test]
fn t02_bracket_layers_and_colors() {
    let r = bracket();
    let names: Vec<&str> = r.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Outline", "Center", "Hidden", "Title"]);
    let colors: Vec<i16> = r.layers.iter().map(|l| l.color).collect();
    assert_eq!(colors, [7, 1, 2, 7]);
}

#[test]
fn t03_bracket_ignored_report() {
    let expected: BTreeMap<String, usize> =
        [("DIMENSION", 6), ("HATCH", 4), ("MTEXT", 1), ("TEXT", 6)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
    assert_eq!(bracket().ignored, expected);
}

#[test]
fn t04_bracket_outline_contours_and_circles() {
    let r = bracket();
    let o = r.layer("Outline").unwrap();
    assert_eq!(o.contours.len(), 12);
    assert_eq!(o.contours.iter().filter(|c| c.closed).count(), 10);
    let mut radii: Vec<f64> = o.contours.iter().filter_map(circle_radius).collect();
    radii.sort_by(f64::total_cmp);
    let expected = [7.9375, 7.9375, 7.9375, 7.9375, 22.225, 38.1];
    assert_eq!(radii.len(), 6);
    assert!(
        radii.iter().zip(expected).all(|(a, b)| close(*a, b, T)),
        "{radii:?}"
    );
    let b = o
        .contours
        .iter()
        .fold(hc_geom::BBox::empty(), |b, c| b.union(&c.bbox()));
    assert!(
        b.min.approx_eq(pt(25.4, 25.4), T) && b.max.approx_eq(pt(330.2, 152.4), T),
        "{b:?}"
    );
}

#[test]
fn t05_bracket_outer_rectangle_and_slot() {
    let r = bracket();
    let o = r.layer("Outline").unwrap();
    let outer = o.contours.iter().find(|c| c.segments.len() == 8).unwrap();
    let lines = outer
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::Line { .. }))
        .count();
    assert_eq!(lines, 4);
    assert!(outer
        .segments
        .iter()
        .all(|s| !matches!(s, Segment::Arc { radius, .. } if !close(*radius, 12.7, T))));
    assert!(close(outer.signed_area().abs(), 25667.947, 1e-3));
    assert!(close(outer.length(), 638.596, 1e-3));

    let slot = o
        .contours
        .iter()
        .find(|c| {
            c.closed
                && c.segments.len() == 4
                && c.segments.iter().any(|s| matches!(s, Segment::Arc { .. }))
        })
        .unwrap();
    assert!(close(slot.signed_area().abs(), 597.201, 1e-3));
    let b = slot.bbox();
    assert!(close(b.min.x, 96.52, T) && close(b.max.x, 157.48, T));
    assert!(close(b.min.y, 38.1, T) && close(b.max.y, 48.26, T));
}

#[test]
fn t06_bracket_containment_depths() {
    let r = bracket();
    let closed: Vec<Contour> = r
        .layer("Outline")
        .unwrap()
        .contours
        .iter()
        .filter(|c| c.closed)
        .cloned()
        .collect();
    let tree = containment_tree(&closed);
    let count = |d: usize| tree.iter().filter(|n| n.depth == d).count();
    assert_eq!((count(0), count(1), count(2)), (3, 6, 1));
    let inner = tree.iter().find(|n| n.depth == 2).unwrap();
    assert!(close(
        circle_radius(&closed[inner.contour]).unwrap(),
        22.225,
        T
    ));
    assert!(close(
        circle_radius(&closed[inner.parent.unwrap()]).unwrap(),
        38.1,
        T
    ));
}

#[test]
fn t07_bracket_center_hidden_title() {
    let r = bracket();
    let c = r.layer("Center").unwrap();
    assert_eq!(c.contours.len(), 11);
    assert!(c.contours.iter().all(|c| !c.closed));
    let h = r.layer("Hidden").unwrap();
    assert_eq!(h.contours.len(), 1);
    assert!(close(circle_radius(&h.contours[0]).unwrap(), 28.575, T));
    let t = r.layer("Title").unwrap();
    assert_eq!(t.contours.iter().filter(|c| c.closed).count(), 2);
    assert_eq!(t.contours.iter().filter(|c| !c.closed).count(), 3);
}

#[test]
fn t08_bracket_total_bbox_and_circles() {
    let r = bracket();
    let b = r.bbox();
    assert!(
        b.min.approx_eq(pt(-12.7, -12.7), T) && b.max.approx_eq(pt(457.2, 190.5), T),
        "{b:?}"
    );
    let circles = r
        .layers
        .iter()
        .flat_map(|l| &l.contours)
        .filter_map(circle_radius)
        .count();
    assert_eq!(circles, 7);
}

// ---------------------------------------------------------------- Synthetic

#[test]
fn t09_units() {
    for (u, len) in [
        (Units::Millimeters, 10.0),
        (Units::Centimeters, 100.0),
        (Units::Meters, 10000.0),
        (Units::Feet, 3048.0),
    ] {
        let mut d = new_drawing();
        d.header.default_drawing_units = u;
        add(&mut d, "0", line(0.0, 0.0, 10.0, 0.0));
        let r = import(&d);
        assert!(close(r.layers[0].contours[0].length(), len, T), "{u:?}");
        assert!(r.warnings.is_empty());
    }
    // R12 does not write $INSUNITS at all
    let mut d = new_drawing();
    d.header.version = AcadVersion::R12;
    add(&mut d, "0", line(0.0, 0.0, 10.0, 0.0));
    let r = import(&d);
    assert!(close(r.layers[0].contours[0].length(), 10.0, T));
    assert_eq!(r.warnings, [ImportWarning::UnitsAssumedMm { insunits: 0 }]);
}

#[test]
fn t10_arc_sweep() {
    let mut d = new_drawing();
    add(
        &mut d,
        "A",
        EntityType::Arc(Arc::new(dpt(0.0, 0.0), 5.0, 0.0, 90.0)),
    );
    add(
        &mut d,
        "B",
        EntityType::Arc(Arc::new(dpt(0.0, 0.0), 5.0, 270.0, 90.0)),
    );
    let r = import(&d);
    let sweep = |l: &str| {
        let c = &r.layer(l).unwrap().contours;
        assert_eq!(c.len(), 1);
        assert!(!c[0].closed);
        match c[0].segments[0] {
            Segment::Arc { sweep, .. } => sweep,
            _ => panic!("not an arc"),
        }
    };
    assert!(close(sweep("A"), FRAC_PI_2, 1e-9));
    assert!(close(sweep("B"), PI, 1e-9));
}

#[test]
fn t11_shuffled_square_lines() {
    let mut d = new_drawing();
    add(&mut d, "A", line(10.0, 10.0, 0.0, 10.0));
    add(&mut d, "A", line(0.0, 0.0, 10.0, 0.0));
    add(&mut d, "A", line(0.0, 0.0, 0.0, 10.0));
    add(&mut d, "A", line(10.0, 0.0, 10.0, 10.0));
    let r = import(&d);
    let c = &r.layer("A").unwrap().contours;
    assert_eq!(c.len(), 1);
    assert!(c[0].closed);
    assert!(close(c[0].signed_area().abs(), 100.0, T));
}

fn lwpoly_circle(normal_z: f64) -> EntityType {
    let p = LwPolyline {
        flags: 1,
        extrusion_direction: Vector::new(0.0, 0.0, normal_z),
        vertices: vec![
            LwPolylineVertex {
                x: 0.0,
                y: 0.0,
                bulge: 1.0,
                ..Default::default()
            },
            LwPolylineVertex {
                x: 10.0,
                y: 0.0,
                bulge: 1.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    EntityType::LwPolyline(p)
}

#[test]
fn t12_lwpolyline_bulge_circle_and_minus_z() {
    let mut d = new_drawing();
    add(&mut d, "P", lwpoly_circle(1.0));
    add(&mut d, "M", lwpoly_circle(-1.0));
    let r = import(&d);
    let c = &r.layer("P").unwrap().contours;
    assert_eq!(c.len(), 1);
    assert!(c[0].closed);
    assert_eq!(c[0].segments.len(), 2);
    assert!(c[0]
        .segments
        .iter()
        .all(|s| matches!(s, Segment::Arc { .. })));
    assert!(close(c[0].signed_area(), 25.0 * PI, T));
    let m = &r.layer("M").unwrap().contours[0];
    let b = m.bbox();
    assert!(close(b.min.x, -10.0, T) && close(b.max.x, 0.0, T));
    assert!(close(m.signed_area().abs(), 25.0 * PI, T));
}

#[test]
fn t13_extrusion_circle_and_line() {
    let mut d = new_drawing();
    let mut c = Circle::new(dpt(3.0, 4.0), 2.0);
    c.normal = Vector::new(0.0, 0.0, -1.0);
    add(&mut d, "C", EntityType::Circle(c));
    let mut l = Line::new(dpt(1.0, 1.0), dpt(2.0, 1.0));
    l.extrusion_direction = Vector::new(0.0, 0.0, -1.0);
    add(&mut d, "L", EntityType::Line(l));
    let mut tilted = Circle::new(dpt(0.0, 0.0), 1.0);
    tilted.normal = Vector::new(1.0, 0.0, 0.0);
    add(&mut d, "T", EntityType::Circle(tilted));
    let r = import(&d);
    let Segment::Arc { center, .. } = r.layer("C").unwrap().contours[0].segments[0] else {
        panic!("not an arc")
    };
    assert!(center.approx_eq(pt(-3.0, 4.0), T));
    let l = &r.layer("L").unwrap().contours[0];
    assert!(l.start().approx_eq(pt(1.0, 1.0), T) && l.end().approx_eq(pt(2.0, 1.0), T));
    assert!(r.layer("T").is_none());
    assert_eq!(
        r.warnings,
        [ImportWarning::UnsupportedExtrusion {
            entity: "CIRCLE".into()
        }]
    );
}

#[test]
fn t14_polyline_2d_and_3d() {
    let mut d = new_drawing();
    for (flags, layer) in [(1, "P2"), (8, "P3")] {
        let mut p = Polyline {
            flags,
            ..Default::default()
        };
        for (x, y) in [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)] {
            p.add_vertex(&mut d, Vertex::new(dpt(x, y)));
        }
        add(&mut d, layer, EntityType::Polyline(p));
    }
    let r = import(&d);
    let c = &r.layer("P2").unwrap().contours;
    assert_eq!(c.len(), 1);
    assert!(c[0].closed);
    assert!(close(c[0].signed_area().abs(), 25.0, T));
    assert!(r.layer("P3").is_none());
    assert_eq!(r.ignored.get("POLYLINE (3D/mesh)"), Some(&1));
}

fn ellipse(ratio: f64, start: f64, end: f64) -> EntityType {
    EntityType::Ellipse(Ellipse {
        center: dpt(0.0, 0.0),
        major_axis: Vector::new(10.0, 0.0, 0.0),
        normal: Vector::new(0.0, 0.0, 1.0),
        minor_axis_ratio: ratio,
        start_parameter: start,
        end_parameter: end,
    })
}

#[test]
fn t15_ellipse() {
    let mut d = new_drawing();
    add(&mut d, "E", ellipse(0.5, 0.0, 2.0 * PI));
    add(&mut d, "R", ellipse(1.0, 0.0, 2.0 * PI));
    add(&mut d, "H", ellipse(0.5, 0.0, PI));
    let r = import(&d);

    let e = &r.layer("E").unwrap().contours;
    assert_eq!(e.len(), 1);
    let e = &e[0];
    assert!(e.closed && all_lines(e));
    for s in &e.segments {
        let (a, b) = (s.start(), s.end());
        assert!(close((a.x / 10.0).powi(2) + (a.y / 5.0).powi(2), 1.0, 1e-9));
        // true curve point at the mid parameter of the chord
        let ta = (a.y / 5.0).atan2(a.x / 10.0);
        let mut tb = (b.y / 5.0).atan2(b.x / 10.0);
        if tb < ta {
            tb += 2.0 * PI;
        }
        let tm = 0.5 * (ta + tb);
        let m = pt(10.0 * tm.cos(), 5.0 * tm.sin());
        let ab = b - a;
        let dev = ab.cross(m - a).abs() / ab.length();
        assert!(dev <= 0.01, "chord deviation {dev}");
    }
    // spec n formula: 71 chords; amendment A1: |area| within 0.2 % of 50π
    assert_eq!(e.segments.len(), 71);
    assert!(close(e.signed_area().abs(), 50.0 * PI, 50.0 * PI * 0.002));

    let rc = &r.layer("R").unwrap().contours[0];
    assert!(rc.closed);
    assert!(close(circle_radius(rc).unwrap(), 10.0, T));

    let h = &r.layer("H").unwrap().contours;
    assert_eq!(h.len(), 1);
    assert!(!h[0].closed);
    let (s, en) = (h[0].start(), h[0].end());
    let ends_ok = (s.approx_eq(pt(10.0, 0.0), T) && en.approx_eq(pt(-10.0, 0.0), T))
        || (s.approx_eq(pt(-10.0, 0.0), T) && en.approx_eq(pt(10.0, 0.0), T));
    assert!(ends_ok, "{s:?} {en:?}");
}

fn spline(degree: i32, ctrl: &[(f64, f64)], knots: &[f64]) -> EntityType {
    EntityType::Spline(Spline {
        degree_of_curve: degree,
        control_points: ctrl.iter().map(|&(x, y)| dpt(x, y)).collect(),
        knot_values: knots.to_vec(),
        ..Default::default()
    })
}

#[test]
fn t16_spline() {
    let mut d = new_drawing();
    add(
        &mut d,
        "D1",
        spline(
            1,
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
            &[0.0, 0.0, 1.0, 2.0, 2.0],
        ),
    );
    add(
        &mut d,
        "D2",
        spline(
            2,
            &[(0.0, 0.0), (5.0, 10.0), (10.0, 0.0)],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ),
    );
    let fit = Spline {
        degree_of_curve: 3,
        fit_points: vec![dpt(0.0, 0.0), dpt(1.0, 1.0), dpt(2.0, 0.0)],
        ..Default::default()
    };
    add(&mut d, "F", EntityType::Spline(fit));
    let r = import(&d);

    let c = &r.layer("D1").unwrap().contours;
    assert_eq!(c.len(), 1);
    assert!(!c[0].closed && all_lines(&c[0]));
    let mut verts: Vec<Point> = c[0].segments.iter().map(|s| s.start()).collect();
    verts.push(c[0].end());
    for v in [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)] {
        assert!(verts.iter().any(|p| p.approx_eq(v, T)), "missing {v:?}");
    }
    // every vertex lies on one of the two legs
    assert!(verts
        .iter()
        .all(|p| close(p.y, 0.0, T) || close(p.x, 10.0, T)));

    let c = &r.layer("D2").unwrap().contours[0];
    assert!(c.start().approx_eq(pt(0.0, 0.0), T) && c.end().approx_eq(pt(10.0, 0.0), T));
    let curve = |u: f64| pt(10.0 * u, 20.0 * u * (1.0 - u));
    let verts: Vec<Point> = c.segments.iter().map(|s| s.start()).collect();
    assert!(verts.iter().any(|p| p.approx_eq(pt(5.0, 5.0), T)));
    for s in &c.segments {
        let (a, b) = (s.start(), s.end());
        assert!(curve(a.x / 10.0).approx_eq(a, T)); // sample on the curve
        let m = curve((a.x + b.x) / 20.0);
        let ab = b - a;
        assert!(ab.cross(m - a).abs() / ab.length() <= 0.01);
    }

    assert!(r
        .warnings
        .contains(&ImportWarning::SplineFitPointsOnly { layer: "F".into() }));
    assert_eq!(r.layer("F").unwrap().contours.len(), 1);
}

#[test]
fn t17_point() {
    let mut d = new_drawing();
    add(
        &mut d,
        "Drill",
        EntityType::ModelPoint(ModelPoint::new(dpt(5.0, 5.0))),
    );
    let r = import(&d);
    let l = r.layer("Drill").unwrap();
    assert_eq!(l.points, [pt(5.0, 5.0)]);
    assert!(l.contours.is_empty());
}

fn block_b(d: &mut Drawing) {
    let mut l = Entity::new(line(1.0, 1.0, 2.0, 1.0));
    l.common.layer = "0".into();
    let mut c = Entity::new(EntityType::Circle(Circle::new(dpt(1.0, 1.0), 1.0)));
    c.common.layer = "X".into();
    d.add_block(Block {
        name: "B".into(),
        base_point: dpt(1.0, 1.0),
        entities: vec![l, c],
        ..Default::default()
    });
}

fn insert(name: &str, at: (f64, f64), rot: f64, sx: f64, sy: f64) -> EntityType {
    EntityType::Insert(Insert {
        name: name.into(),
        location: dpt(at.0, at.1),
        rotation: rot,
        x_scale_factor: sx,
        y_scale_factor: sy,
        ..Default::default()
    })
}

fn insert_b(rot: f64, sx: f64, sy: f64) -> DxfImport {
    let mut d = new_drawing();
    block_b(&mut d);
    add(&mut d, "L", insert("B", (10.0, 0.0), rot, sx, sy));
    import(&d)
}

#[test]
fn t18_insert_explode() {
    let r = insert_b(90.0, 2.0, 2.0);
    let l = &r.layer("L").unwrap().contours[0];
    assert!(l.start().approx_eq(pt(10.0, 0.0), T) && l.end().approx_eq(pt(10.0, 2.0), T));
    let Segment::Arc { center, radius, .. } = r.layer("X").unwrap().contours[0].segments[0] else {
        panic!("not an arc")
    };
    assert!(center.approx_eq(pt(10.0, 0.0), T) && close(radius, 2.0, T));

    let r = insert_b(0.0, -1.0, 1.0);
    let l = &r.layer("L").unwrap().contours[0];
    assert!(l.start().approx_eq(pt(10.0, 0.0), T) && l.end().approx_eq(pt(9.0, 0.0), T));
    let Segment::Arc {
        center,
        radius,
        sweep,
        ..
    } = r.layer("X").unwrap().contours[0].segments[0]
    else {
        panic!("not an arc")
    };
    assert!(center.approx_eq(pt(10.0, 0.0), T) && close(radius, 1.0, T) && sweep < 0.0);

    let r = insert_b(0.0, 2.0, 1.0);
    let l = &r.layer("L").unwrap().contours[0];
    assert_eq!((l.start(), l.end()), (pt(10.0, 0.0), pt(12.0, 0.0)));
    let c = &r.layer("X").unwrap().contours[0];
    assert!(c.closed && all_lines(c));
    let b = c.bbox();
    assert!(close(b.min.x, 8.0, 0.01) && close(b.max.x, 12.0, 0.01));
    assert!(close(b.min.y, -1.0, 0.01) && close(b.max.y, 1.0, 0.01));
}

#[test]
fn t19_nested_selfref_missing_insert() {
    let mut d = new_drawing();
    d.add_block(Block {
        name: "INNER".into(),
        entities: vec![Entity::new(line(0.0, 0.0, 1.0, 0.0))],
        ..Default::default()
    });
    d.add_block(Block {
        name: "OUTER".into(),
        entities: vec![Entity::new(insert("INNER", (5.0, 0.0), 0.0, 1.0, 1.0))],
        ..Default::default()
    });
    d.add_block(Block {
        name: "SELF".into(),
        entities: vec![Entity::new(insert("SELF", (1.0, 0.0), 0.0, 1.0, 1.0))],
        ..Default::default()
    });
    add(&mut d, "N", insert("OUTER", (10.0, 0.0), 90.0, 1.0, 1.0));
    add(&mut d, "S", insert("SELF", (0.0, 0.0), 0.0, 1.0, 1.0));
    add(&mut d, "M", insert("NOPE", (0.0, 0.0), 0.0, 1.0, 1.0));
    let r = import(&d);
    let l = &r.layer("N").unwrap().contours[0];
    assert!(l.start().approx_eq(pt(10.0, 5.0), T) && l.end().approx_eq(pt(10.0, 6.0), T));
    assert!(r.warnings.contains(&ImportWarning::InsertNestingTooDeep {
        block: "SELF".into()
    }));
    assert!(r.warnings.contains(&ImportWarning::MissingBlock {
        name: "NOPE".into()
    }));
}

#[test]
fn t20_hatch_and_text_ignored() {
    let src = "0\nSECTION\n2\nENTITIES\n\
0\nHATCH\n8\n0\n\
0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n1\nhi\n\
0\nTEXT\n8\n0\n10\n0\n20\n5\n40\n1\n1\nho\n\
0\nLINE\n8\n0\n10\n0\n20\n0\n11\n3\n21\n0\n\
0\nENDSEC\n0\nEOF\n";
    let r = import_dxf_bytes(src.as_bytes(), &ImportOptions::default()).unwrap();
    let expected: BTreeMap<String, usize> =
        [("HATCH".to_string(), 1), ("TEXT".to_string(), 2)].into();
    assert_eq!(r.ignored, expected);
    let c = &r.layer("0").unwrap().contours;
    assert_eq!(c.len(), 1);
    assert!(close(c[0].length(), 3.0, T));
}

#[test]
fn t21_garbage_and_missing_file() {
    assert!(matches!(
        import_dxf_bytes(b"garbage", &ImportOptions::default()),
        Err(DxfError::Parse(_))
    ));
    assert!(matches!(
        import_dxf_file(
            Path::new("/nonexistent/nope.dxf"),
            &ImportOptions::default()
        ),
        Err(DxfError::Io(_))
    ));
}

#[test]
fn t22_paper_space_skipped() {
    let mut d = new_drawing();
    let mut e = Entity::new(line(0.0, 0.0, 1.0, 0.0));
    e.common.is_in_paper_space = true;
    d.add_entity(e);
    add(&mut d, "M", line(0.0, 0.0, 2.0, 0.0));
    let r = import(&d);
    assert_eq!(r.layers.len(), 1);
    assert_eq!(r.layers[0].name, "M");
    assert!(r
        .warnings
        .contains(&ImportWarning::PaperSpaceSkipped { count: 1 }));
}

#[test]
fn empty_drawing_is_ok() {
    let r = import(&new_drawing());
    assert!(r.layers.is_empty() && r.warnings.is_empty());
}

// ---------------------------------------------------------------- architect review

#[test]
fn r1_block_inserting_itself_three_times_is_fast() {
    let mut d = new_drawing();
    let mut ents = vec![Entity::new(line(0.0, 0.0, 1.0, 0.0))];
    for x in [1.0, 2.0, 3.0] {
        ents.push(Entity::new(insert("S", (x, 0.0), 0.0, 1.0, 1.0)));
    }
    d.add_block(Block {
        name: "S".into(),
        entities: ents,
        ..Default::default()
    });
    add(&mut d, "L", insert("S", (0.0, 0.0), 0.0, 1.0, 1.0));
    let t0 = std::time::Instant::now();
    let r = import(&d);
    assert!(t0.elapsed().as_secs_f64() < 1.0);
    let w: Vec<_> = r
        .warnings
        .iter()
        .filter(|w| matches!(w, ImportWarning::InsertNestingTooDeep { .. }))
        .collect();
    assert_eq!(
        w,
        [&ImportWarning::InsertNestingTooDeep { block: "S".into() }]
    );
    // only the first level is expanded: one line
    assert_eq!(r.layer("L").unwrap().contours.len(), 1);
}

#[test]
fn r2_invalid_splines_are_counted() {
    let mut d = new_drawing();
    // 3 control points, degree 2, but only 4 knots (needs 6)
    add(
        &mut d,
        "S",
        spline(
            2,
            &[(0.0, 0.0), (5.0, 10.0), (10.0, 0.0)],
            &[0.0, 0.0, 1.0, 1.0],
        ),
    );
    let one_fit = Spline {
        fit_points: vec![dpt(1.0, 1.0)],
        ..Default::default()
    };
    add(&mut d, "S", EntityType::Spline(one_fit));
    let r = import(&d);
    assert_eq!(r.ignored.get("SPLINE (invalid)"), Some(&2));
    assert!(r.layer("S").is_none());
}

#[test]
fn ellipse_half_with_minus_z_normal_runs_below() {
    let mut d = new_drawing();
    let EntityType::Ellipse(mut e) = ellipse(0.5, 0.0, PI) else {
        unreachable!()
    };
    e.normal = Vector::new(0.0, 0.0, -1.0);
    add(&mut d, "E", EntityType::Ellipse(e));
    let r = import(&d);
    let c = &r.layer("E").unwrap().contours[0];
    assert!(!c.closed);
    assert!(c.start().approx_eq(pt(10.0, 0.0), T) && c.end().approx_eq(pt(-10.0, 0.0), T));
    let b = c.bbox();
    assert!(close(b.min.y, -5.0, T) && close(b.max.y, 0.0, T));
    assert!(c
        .segments
        .iter()
        .any(|s| s.start().approx_eq(pt(0.0, -5.0), T)));
}
