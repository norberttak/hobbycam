//! Tester edge cases for M2 (public API only).

use dxf::entities::*;
use dxf::enums::{AcadVersion, Units};
use dxf::{Block, Drawing, LwPolylineVertex, Vector};
use hc_dxf::*;
use hc_geom::{Contour, Point, Segment};
use std::f64::consts::{FRAC_PI_2, PI};
use std::path::Path;

const T: f64 = 1e-6;
const BRACKET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dxf/autocad2018_bracket_inch.dxf"
);

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn dpt(x: f64, y: f64) -> dxf::Point {
    dxf::Point::new(x, y, 0.0)
}

fn new_drawing() -> Drawing {
    let mut d = Drawing::new();
    d.header.version = AcadVersion::R2018;
    d.header.default_drawing_units = Units::Millimeters;
    d
}

fn ent(layer: &str, specific: EntityType) -> Entity {
    let mut e = Entity::new(specific);
    e.common.layer = layer.to_string();
    e
}

fn add(d: &mut Drawing, layer: &str, specific: EntityType) {
    d.add_entity(ent(layer, specific));
}

fn import(d: &Drawing) -> DxfImport {
    let mut buf = Vec::new();
    d.save(&mut buf).unwrap();
    import_dxf_bytes(&buf, &ImportOptions::default()).unwrap()
}

fn import_str(s: &str) -> Result<DxfImport, DxfError> {
    import_dxf_bytes(s.as_bytes(), &ImportOptions::default())
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> EntityType {
    EntityType::Line(Line::new(dpt(x0, y0), dpt(x1, y1)))
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

fn all_lines(c: &Contour) -> bool {
    c.segments.iter().all(|s| matches!(s, Segment::Line { .. }))
}

fn finite(c: &Contour) -> bool {
    c.segments.iter().all(|s| match *s {
        Segment::Line { start, end } => {
            start.x.is_finite() && start.y.is_finite() && end.x.is_finite() && end.y.is_finite()
        }
        Segment::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => {
            center.x.is_finite()
                && center.y.is_finite()
                && radius.is_finite()
                && start_angle.is_finite()
                && sweep.is_finite()
        }
    })
}

fn total_length(r: &DxfImport) -> f64 {
    r.layers
        .iter()
        .flat_map(|l| &l.contours)
        .map(Contour::length)
        .sum()
}

fn arc_parts(s: &Segment) -> (Point, f64, f64) {
    match *s {
        Segment::Arc {
            center,
            radius,
            sweep,
            ..
        } => (center, radius, sweep),
        _ => panic!("not an arc: {s:?}"),
    }
}

/// Wrap ENTITIES body (group code / value lines) into a minimal DXF, optional $INSUNITS.
fn mini_dxf(insunits: Option<i16>, entities: &str) -> String {
    let header = insunits
        .map(|u| format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{u}\n0\nENDSEC\n"))
        .unwrap_or_default();
    format!("{header}0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
}

const LINE_0_10: &str = "0\nLINE\n8\nA\n10\n0\n20\n0\n11\n10\n21\n0\n";

// ---------------------------------------------------------------- Bracket cross-checks

#[test]
fn bracket_totals_match_raw_dxf() {
    // independently computed from the raw DXF text (16 LINE, 7 CIRCLE, 6 LWPOLYLINE with bulges)
    let r = import_dxf_file(Path::new(BRACKET), &ImportOptions::default()).unwrap();
    let segs: usize = r
        .layers
        .iter()
        .flat_map(|l| &l.contours)
        .map(|c| c.segments.len())
        .sum();
    assert_eq!(segs, 51);
    assert!(close(total_length(&r), 4496.901342072871, 1e-6));
    // every contour is finite and contiguous
    for c in r.layers.iter().flat_map(|l| &l.contours) {
        assert!(finite(c));
        for w in c.segments.windows(2) {
            assert!(w[0].end().approx_eq(w[1].start(), 1e-6));
        }
    }
}

#[test]
fn bracket_lf_and_crlf_identical() {
    let crlf = std::fs::read(BRACKET).unwrap();
    assert!(
        crlf.windows(2).any(|w| w == b"\r\n"),
        "fixture should be CRLF"
    );
    let lf: Vec<u8> = crlf.iter().copied().filter(|&b| b != b'\r').collect();
    let o = ImportOptions::default();
    let a = import_dxf_bytes(&crlf, &o).unwrap();
    let b = import_dxf_bytes(&lf, &o).unwrap();
    assert_eq!(a, b);
}

// ---------------------------------------------------------------- OCS

#[test]
fn arc_minus_z_matches_hand_mirrored_arc() {
    // OCS arc center (3,4) r2, 0°..90° with normal -Z: OCS x-axis = -X, so in WCS it is
    // center (-3,4) from (-5,4) to (-3,6), clockwise. The same curve drawn with +Z is
    // center (-3,4), 90°..180° (traversed the other way).
    let mut d = new_drawing();
    let mut a = Arc::new(dpt(3.0, 4.0), 2.0, 0.0, 90.0);
    a.normal = Vector::new(0.0, 0.0, -1.0);
    add(&mut d, "M", EntityType::Arc(a));
    add(
        &mut d,
        "P",
        EntityType::Arc(Arc::new(dpt(-3.0, 4.0), 2.0, 90.0, 180.0)),
    );
    let r = import(&d);
    let m = &r.layer("M").unwrap().contours[0];
    let p = &r.layer("P").unwrap().contours[0];
    let (mc, mr, ms) = arc_parts(&m.segments[0]);
    let (pc, pr, ps) = arc_parts(&p.segments[0]);
    assert!(mc.approx_eq(pc, T) && mc.approx_eq(pt(-3.0, 4.0), T));
    assert!(close(mr, pr, T) && close(mr, 2.0, T));
    assert!(close(ms, -FRAC_PI_2, 1e-9), "mirrored sweep {ms}");
    assert!(close(ps, FRAC_PI_2, 1e-9));
    assert!(m.start().approx_eq(pt(-5.0, 4.0), T) && m.end().approx_eq(pt(-3.0, 6.0), T));
    assert!(m.start().approx_eq(p.end(), T) && m.end().approx_eq(p.start(), T));
    let (bm, bp) = (m.bbox(), p.bbox());
    assert!(bm.min.approx_eq(bp.min, T) && bm.max.approx_eq(bp.max, T));
    assert!(close(m.length(), p.length(), T));
}

#[test]
fn lwpolyline_minus_z_with_open_bulge_arc() {
    // open LWPOLYLINE (0,0) -b=1-> (10,0) with -Z: CCW lower semicircle in OCS becomes
    // the mirror image: (0,0)->(-10,0) passing (-5,-5), clockwise
    let mut d = new_drawing();
    let p = LwPolyline {
        extrusion_direction: Vector::new(0.0, 0.0, -1.0),
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
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    add(&mut d, "M", EntityType::LwPolyline(p));
    let r = import(&d);
    let c = &r.layer("M").unwrap().contours[0];
    assert!(!c.closed);
    assert!(c.start().approx_eq(pt(0.0, 0.0), T) && c.end().approx_eq(pt(-10.0, 0.0), T));
    let b = c.bbox();
    assert!(b.min.approx_eq(pt(-10.0, -5.0), T) && b.max.approx_eq(pt(0.0, 0.0), T));
    let (_, _, sweep) = arc_parts(&c.segments[0]);
    assert!(close(sweep, -PI, 1e-9));
}

// ---------------------------------------------------------------- LWPOLYLINE

fn v(x: f64, y: f64, bulge: f64) -> LwPolylineVertex {
    LwPolylineVertex {
        x,
        y,
        bulge,
        ..Default::default()
    }
}

#[test]
fn lwpolyline_negative_bulges_closed_with_duplicates() {
    // two CW semicircles (bulge -1) = CW circle center (5,0) r5;
    // duplicate vertices, one of them carrying a non-zero bulge (zero-chord arc)
    let mut d = new_drawing();
    let p = LwPolyline {
        flags: 1,
        vertices: vec![
            v(0.0, 0.0, 0.5), // duplicate with bulge -> zero chord
            v(0.0, 0.0, -1.0),
            v(10.0, 0.0, 0.0), // duplicate with bulge 0
            v(10.0, 0.0, -1.0),
        ],
        ..Default::default()
    };
    add(&mut d, "D", EntityType::LwPolyline(p));
    let r = import(&d);
    let cs = &r.layer("D").unwrap().contours;
    assert_eq!(cs.len(), 1);
    let c = &cs[0];
    assert!(finite(c), "{c:?}");
    assert!(c.closed && c.is_closed_within(T));
    assert_eq!(c.segments.len(), 2);
    assert!(close(c.signed_area(), -25.0 * PI, 1e-6));
    let b = c.bbox();
    assert!(b.min.approx_eq(pt(0.0, -5.0), T) && b.max.approx_eq(pt(10.0, 5.0), T));
    assert!(r.bbox().min.x.is_finite() && r.bbox().max.y.is_finite());
}

#[test]
fn lwpolyline_all_vertices_identical_no_nan() {
    let mut d = new_drawing();
    let p = LwPolyline {
        flags: 1,
        vertices: vec![v(1.0, 1.0, 1.0), v(1.0, 1.0, -0.5), v(1.0, 1.0, 0.0)],
        ..Default::default()
    };
    add(&mut d, "Z", EntityType::LwPolyline(p));
    add(&mut d, "K", line(0.0, 0.0, 1.0, 0.0));
    let r = import(&d);
    // fully degenerate polyline yields no geometry (and no NaN anywhere)
    for c in r.layers.iter().flat_map(|l| &l.contours) {
        assert!(finite(c), "{c:?}");
    }
    assert!(r.layer("Z").is_none_or(|l| l.contours.is_empty()));
    let b = r.bbox();
    assert!(b.min.approx_eq(pt(0.0, 0.0), T) && b.max.approx_eq(pt(1.0, 0.0), T));
}

// ---------------------------------------------------------------- INSERT

/// INNER: LINE (0,0)-(2,0) on "0", ARC c(1,0) r1 0..90° on "ARC".
fn nested_drawing(outer_rot: f64, outer_sx: f64, outer_sy: f64, inner_layer: &str) -> DxfImport {
    let mut d = new_drawing();
    d.add_block(Block {
        name: "INNER".into(),
        entities: vec![
            ent("0", line(0.0, 0.0, 2.0, 0.0)),
            ent(
                "ARC",
                EntityType::Arc(Arc::new(dpt(1.0, 0.0), 1.0, 0.0, 90.0)),
            ),
        ],
        ..Default::default()
    });
    d.add_block(Block {
        name: "OUTER".into(),
        entities: vec![ent(
            inner_layer,
            insert("INNER", (3.0, 0.0), 90.0, 2.0, 2.0),
        )],
        ..Default::default()
    });
    add(
        &mut d,
        "N",
        insert("OUTER", (10.0, 10.0), outer_rot, outer_sx, outer_sy),
    );
    import(&d)
}

#[test]
fn nested_insert_rotation_and_uniform_scale() {
    // world = R(90)·0.5 ∘ (R(90)·2 + (3,0)) + (10,10) -> net rotation 180°, scale 1
    let r = nested_drawing(90.0, 0.5, 0.5, "0");
    let l = &r
        .layer("N")
        .expect("layer-0 line inherits outer INSERT layer")
        .contours;
    assert_eq!(l.len(), 1);
    let (s, e) = (l[0].start(), l[0].end());
    assert!(
        s.approx_eq(pt(10.0, 11.5), T) && e.approx_eq(pt(8.0, 11.5), T),
        "{s:?} {e:?}"
    );

    let a = &r.layer("ARC").unwrap().contours[0];
    let (c, rad, sweep) = arc_parts(&a.segments[0]);
    assert!(c.approx_eq(pt(9.0, 11.5), T) && close(rad, 1.0, T));
    assert!(close(sweep, FRAC_PI_2, 1e-9));
    assert!(a.start().approx_eq(pt(8.0, 11.5), T) && a.end().approx_eq(pt(9.0, 10.5), T));
}

#[test]
fn nested_insert_layer_inheritance_stops_at_named_layer() {
    // inner INSERT on layer "K": the layer-0 line inside INNER goes to "K", not "N"
    let r = nested_drawing(90.0, 0.5, 0.5, "K");
    assert!(r.layer("K").is_some_and(|l| l.contours.len() == 1));
    assert!(r.layer("N").is_none());
    assert!(r.layer("0").is_none());
}

#[test]
fn layer_zero_point_in_block_inherits_insert_layer() {
    let mut d = new_drawing();
    d.add_block(Block {
        name: "P".into(),
        entities: vec![ent(
            "0",
            EntityType::ModelPoint(ModelPoint::new(dpt(1.0, 2.0))),
        )],
        ..Default::default()
    });
    add(&mut d, "Drill", insert("P", (10.0, 0.0), 0.0, 1.0, 1.0));
    // INSERT itself on layer "0": contents stay on "0"
    add(&mut d, "0", insert("P", (20.0, 0.0), 0.0, 1.0, 1.0));
    let r = import(&d);
    assert_eq!(r.layer("Drill").unwrap().points, [pt(11.0, 2.0)]);
    assert_eq!(r.layer("0").unwrap().points, [pt(21.0, 2.0)]);
}

#[test]
fn nested_insert_non_uniform_scale_turns_arcs_into_lines() {
    // INNER (rot 90, scale 2, at (3,0)) inside OUTER inserted at (10,10) with scale (2,1), rot 0.
    // INNER-local p -> OUTER-local q = R90(2p) + (3,0) -> world (10 + 2 q.x, 10 + q.y)
    let world = |x: f64, y: f64| {
        let q = (-2.0 * y + 3.0, 2.0 * x);
        pt(10.0 + 2.0 * q.0, 10.0 + q.1)
    };
    let r = nested_drawing(0.0, 2.0, 1.0, "0");
    let l = &r.layer("N").unwrap().contours[0];
    assert!(all_lines(l));
    assert!(l.start().approx_eq(world(0.0, 0.0), T) && l.end().approx_eq(world(2.0, 0.0), T));

    let a = &r.layer("ARC").unwrap().contours[0];
    assert!(!a.closed && all_lines(a) && a.segments.len() > 1);
    assert!(a.start().approx_eq(world(2.0, 0.0), 1e-9));
    assert!(a.end().approx_eq(world(1.0, 1.0), 1e-9));
    // every vertex lies on the image of the unit circle around (1,0):
    // invert world: q.x = (X-10)/2, q.y = Y-10; p = ((q.y)/2, (3 - q.x)/2)
    let mut verts: Vec<Point> = a.segments.iter().map(|s| s.start()).collect();
    verts.push(a.end());
    for w in verts {
        let (qx, qy) = ((w.x - 10.0) / 2.0, w.y - 10.0);
        let p = pt(qy / 2.0, (3.0 - qx) / 2.0);
        assert!(close(p.distance(pt(1.0, 0.0)), 1.0, 1e-9), "{w:?}");
    }
}

#[test]
fn insert_rotation_with_non_uniform_scale_circle_becomes_closed_polyline() {
    let mut d = new_drawing();
    d.add_block(Block {
        name: "C".into(),
        entities: vec![ent(
            "0",
            EntityType::Circle(Circle::new(dpt(0.0, 0.0), 1.0)),
        )],
        ..Default::default()
    });
    add(&mut d, "E", insert("C", (5.0, 5.0), 90.0, 3.0, 1.0));
    let r = import(&d);
    let c = &r.layer("E").unwrap().contours[0];
    assert!(c.closed && all_lines(c));
    // scale x by 3 then rotate 90: ellipse with semi-axes 1 (x) and 3 (y)
    let b = c.bbox();
    assert!(close(b.min.x, 4.0, 0.01) && close(b.max.x, 6.0, 0.01));
    assert!(close(b.min.y, 2.0, 0.01) && close(b.max.y, 8.0, 0.01));
    // true curve (5 + cos t, 5 + 3 sin t): every chord within 0.01 mm of it
    let curve = |t: f64| pt(5.0 + t.cos(), 5.0 + 3.0 * t.sin());
    for s in &c.segments {
        let (a, b) = (s.start(), s.end());
        assert!(close(
            (a.x - 5.0).powi(2) + ((a.y - 5.0) / 3.0).powi(2),
            1.0,
            1e-9
        ));
        let ta = ((a.y - 5.0) / 3.0).atan2(a.x - 5.0);
        let mut tb = ((b.y - 5.0) / 3.0).atan2(b.x - 5.0);
        if tb < ta {
            tb += 2.0 * PI;
        }
        let ab = b - a;
        let dev = (0..=20)
            .map(|k| curve(ta + (tb - ta) * k as f64 / 20.0))
            .map(|m| ab.cross(m - a).abs() / ab.length())
            .fold(0.0, f64::max);
        assert!(dev <= 0.01 + 1e-9, "chord deviation {dev}");
    }
}

// ---------------------------------------------------------------- units

#[test]
fn insunits_hand_written_header() {
    for (code, scale) in [(1, 25.4), (2, 304.8), (4, 1.0), (5, 10.0), (6, 1000.0)] {
        let r = import_str(&mini_dxf(Some(code), LINE_0_10)).unwrap();
        assert_eq!(r.unit_scale, scale, "code {code}");
        assert!(r.warnings.is_empty(), "code {code}: {:?}", r.warnings);
        assert!(close(total_length(&r), 10.0 * scale, 1e-9), "code {code}");
    }
    for code in [0, 3, 14] {
        let r = import_str(&mini_dxf(Some(code), LINE_0_10)).unwrap();
        assert_eq!(r.unit_scale, 1.0);
        assert_eq!(
            r.warnings,
            [ImportWarning::UnitsAssumedMm { insunits: code }]
        );
        assert!(close(total_length(&r), 10.0, 1e-9));
    }
    let r = import_str(&mini_dxf(None, LINE_0_10)).unwrap();
    assert_eq!(r.warnings, [ImportWarning::UnitsAssumedMm { insunits: 0 }]);
}

#[test]
fn feet_circle_radius_and_points_scaled() {
    let mut d = new_drawing();
    d.header.default_drawing_units = Units::Feet;
    add(
        &mut d,
        "C",
        EntityType::Circle(Circle::new(dpt(1.0, 2.0), 0.5)),
    );
    add(
        &mut d,
        "P",
        EntityType::ModelPoint(ModelPoint::new(dpt(1.0, 1.0))),
    );
    let r = import(&d);
    let (c, rad, _) = arc_parts(&r.layer("C").unwrap().contours[0].segments[0]);
    assert!(c.approx_eq(pt(304.8, 609.6), 1e-9) && close(rad, 152.4, 1e-9));
    assert!(r.layer("P").unwrap().points[0].approx_eq(pt(304.8, 304.8), 1e-9));
}

#[test]
fn centimetre_ellipse_flatten_tolerance_in_mm() {
    // ellipse 10 cm x 5 cm: deviation must be <= 0.01 mm after scaling, not 0.01 cm
    let mut d = new_drawing();
    d.header.default_drawing_units = Units::Centimeters;
    add(
        &mut d,
        "E",
        EntityType::Ellipse(Ellipse {
            center: dpt(0.0, 0.0),
            major_axis: Vector::new(10.0, 0.0, 0.0),
            normal: Vector::new(0.0, 0.0, 1.0),
            minor_axis_ratio: 0.5,
            start_parameter: 0.0,
            end_parameter: 2.0 * PI,
        }),
    );
    let r = import(&d);
    let e = &r.layer("E").unwrap().contours[0];
    assert!(e.closed && all_lines(e));
    let (ax, by) = (100.0, 50.0);
    for s in &e.segments {
        let (a, b) = (s.start(), s.end());
        assert!(close((a.x / ax).powi(2) + (a.y / by).powi(2), 1.0, 1e-9));
        let ta = (a.y / by).atan2(a.x / ax);
        let mut tb = (b.y / by).atan2(b.x / ax);
        if tb < ta {
            tb += 2.0 * PI;
        }
        let tm = 0.5 * (ta + tb);
        let m = pt(ax * tm.cos(), by * tm.sin());
        let ab = b - a;
        assert!(ab.cross(m - a).abs() / ab.length() <= 0.01 + 1e-12);
    }
}

// ---------------------------------------------------------------- empty / garbage / CRLF

#[test]
fn empty_entities_section() {
    let r = import_str(&mini_dxf(Some(4), "")).unwrap();
    assert!(r.layers.is_empty() && r.ignored.is_empty() && r.warnings.is_empty());
    let b = r.bbox();
    assert!(b.is_empty());
}

#[test]
fn only_unsupported_entities() {
    let r = import_str(&mini_dxf(Some(4), "0\nHATCH\n8\n0\n0\nMTEXT\n8\n0\n")).unwrap();
    assert!(r.layers.is_empty());
    assert_eq!(r.ignored.get("HATCH"), Some(&1));
    assert_eq!(r.ignored.get("MTEXT"), Some(&1));
}

#[test]
fn garbage_inputs_err_without_panic() {
    let cases: Vec<Vec<u8>> = vec![
        b"garbage".to_vec(),
        b"0\n".to_vec(),
        b"0\nSECTION\n".to_vec(),
        b"0\nSECTION\n2\nENTITIES\n0\nLINE\n10\nabc\n".to_vec(),
        mini_dxf(
            Some(4),
            "0\nLINE\n8\nA\n10\nnot-a-number\n20\n0\n11\n1\n21\n0\n",
        )
        .into_bytes(),
        vec![0xff, 0xfe, 0, 1, 2, 3, 0x80],
        b"AutoCAD Binary DXF\r\n\x1a\0garbage".to_vec(),
    ];
    for c in cases {
        let r = std::panic::catch_unwind(|| import_dxf_bytes(&c, &ImportOptions::default()));
        let r = r.unwrap_or_else(|_| panic!("panicked on {:?}", String::from_utf8_lossy(&c)));
        assert!(
            matches!(r, Err(DxfError::Parse(_))),
            "{:?} -> {r:?}",
            String::from_utf8_lossy(&c)
        );
    }
}

#[test]
fn truncated_bracket_err_without_panic() {
    let b = std::fs::read(BRACKET).unwrap();
    // many cut points, including mid-line and mid-CRLF
    let mut cuts: Vec<usize> = (1..40).map(|i| b.len() * i / 40).collect();
    cuts.extend([4, 5, 6, b.len() - 1, b.len() - 2, b.len() - 10]);
    for n in cuts {
        let r = std::panic::catch_unwind(|| import_dxf_bytes(&b[..n], &ImportOptions::default()))
            .unwrap_or_else(|_| panic!("panicked when truncated at {n}"));
        // the very end (after "0\nEOF") may legitimately still parse
        if n < b.len() - 10 {
            assert!(r.is_err(), "truncated at {n}/{} gave Ok", b.len());
        }
    }
}

#[test]
fn crlf_hand_written_same_as_lf() {
    let body = "0\nHATCH\n8\n0\n0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n1\nhi\n\
0\nARC\n8\nA\n10\n0\n20\n0\n40\n5\n50\n270\n51\n90\n\
0\nLWPOLYLINE\n8\nB\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n42\n1\n";
    let lf = mini_dxf(Some(4), body);
    let crlf = lf.replace('\n', "\r\n");
    let a = import_str(&lf).unwrap();
    let b = import_str(&crlf).unwrap();
    assert_eq!(a, b);
    assert_eq!(b.ignored.get("HATCH"), Some(&1));
    assert_eq!(b.ignored.get("TEXT"), Some(&1));
    assert!(close(
        b.layer("B").unwrap().contours[0].signed_area(),
        25.0 * PI,
        1e-6
    ));
    let (_, _, sweep) = arc_parts(&b.layer("A").unwrap().contours[0].segments[0]);
    assert!(close(sweep, PI, 1e-9));
}

#[test]
fn empty_or_whitespace_input_is_err() {
    // a 0-byte or whitespace-only file is not a valid DXF; importing it as an empty
    // drawing would silently hide a broken/truncated file from the user
    for c in [&b""[..], b" ", b"  ", b"\r\n"] {
        let r = import_dxf_bytes(c, &ImportOptions::default());
        assert!(matches!(r, Err(DxfError::Parse(_))), "{c:?} -> {r:?}");
    }
}

// ---------------------------------------------------------------- large coordinates / T-junction

#[test]
fn very_large_coordinates() {
    // 1e5 inches = 2.54e6 mm
    let b = 1e5;
    let mut d = new_drawing();
    d.header.default_drawing_units = Units::Inches;
    add(&mut d, "S", line(b, b, b + 1.0, b));
    add(&mut d, "S", line(b + 1.0, b + 1.0, b + 1.0, b));
    add(&mut d, "S", line(b + 1.0, b + 1.0, b, b + 1.0));
    add(&mut d, "S", line(b, b, b, b + 1.0));
    add(
        &mut d,
        "C",
        EntityType::Circle(Circle::new(dpt(b, -b), 0.5)),
    );
    let r = import(&d);
    let s = &r.layer("S").unwrap().contours;
    assert_eq!(s.len(), 1);
    assert!(s[0].closed);
    assert!(close(s[0].signed_area().abs(), 645.16, 1e-2)); // shoelace at 2.5e6 mm loses ~1e-3
    let (c, rad, _) = arc_parts(&r.layer("C").unwrap().contours[0].segments[0]);
    assert!(c.approx_eq(pt(2.54e6, -2.54e6), 1e-6) && close(rad, 12.7, 1e-9));
    let bb = r.bbox();
    assert!(close(bb.max.x, 2.54e6 + 25.4, 1e-6) && close(bb.min.y, -2.54e6 - 12.7, 1e-6));
}

#[test]
fn t_junction_no_panic_length_preserved() {
    let mut d = new_drawing();
    add(&mut d, "T", line(0.0, 0.0, 10.0, 0.0));
    add(&mut d, "T", line(10.0, 0.0, 20.0, 0.0));
    add(&mut d, "T", line(10.0, 0.0, 10.0, 10.0));
    // plus a 4-way cross on another layer
    add(&mut d, "X", line(-5.0, 50.0, 5.0, 50.0));
    add(&mut d, "X", line(0.0, 45.0, 0.0, 50.0));
    add(&mut d, "X", line(0.0, 50.0, 0.0, 55.0));
    let r = import(&d);
    for (name, len) in [("T", 30.0), ("X", 20.0)] {
        let cs = &r.layer(name).unwrap().contours;
        let n: usize = cs.iter().map(|c| c.segments.len()).sum();
        assert_eq!(n, 3, "{name}");
        let total: f64 = cs.iter().map(Contour::length).sum();
        assert!(close(total, len, T), "{name}: {total}");
        assert!(cs.iter().all(|c| !c.closed));
        assert!(cs.len() >= 2);
    }
}
