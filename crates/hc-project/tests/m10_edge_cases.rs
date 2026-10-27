//! M10 verifier: format v4 (WorkOrigin::Point), add_drawn / transform_items edge cases.

use hc_geom::{shapes, Point, Segment, Transform};
use hc_project::{Geom, OpKind, Project, ProjectError, WorkOrigin, FORMAT_VERSION};
use std::fs;

const GOLDEN_V3: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/project/small_v3.hcam"
);
const GOLDEN_V4: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/project/small_v4.hcam"
);

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn with_origin(origin_json: &str) -> String {
    let text = Project::new().to_json().unwrap();
    assert!(text.contains("\"origin\": \"drawing_origin\""));
    text.replace(
        "\"origin\": \"drawing_origin\"",
        &format!("\"origin\": {origin_json}"),
    )
}

#[test]
fn v4_point_round_trips_bit_exact_through_a_file() {
    let dir = std::env::temp_dir().join(format!("hc-m10-proj-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("o.hcam");
    for o in [
        p(1.0 / 3.0, -2.0f64.sqrt()),
        p(1e-12, 1e6),
        p(-0.0, 0.0),
        p(f64::MAX, f64::MIN_POSITIVE),
    ] {
        let mut pr = Project::new();
        pr.stock.origin = WorkOrigin::Point(o);
        pr.save(&path).unwrap();
        let (back, problems) = Project::load(&path).unwrap();
        assert!(problems.is_empty());
        assert_eq!(back, pr);
        let WorkOrigin::Point(q) = back.stock.origin else {
            panic!()
        };
        assert_eq!(
            (q.x.to_bits(), q.y.to_bits()),
            (o.x.to_bits(), o.y.to_bits())
        );
    }
}

#[test]
fn v3_file_migrates_to_v4_and_saves_as_v4() {
    let text = fs::read_to_string(GOLDEN_V3).unwrap();
    assert!(text.contains("\"format_version\": 3"));
    let (pr, problems) = Project::from_json(&text).unwrap();
    assert!(problems.is_empty());
    assert_eq!(pr.format_version, FORMAT_VERSION);
    assert_eq!(pr.stock.origin, WorkOrigin::DrawingOrigin);
    let out = pr.to_json().unwrap();
    assert!(out.contains("\"format_version\": 4,"));
    // every preset spelling in a v3 file still loads
    for (s, o) in [
        ("\"stock_center\"", WorkOrigin::StockCenter),
        ("\"stock_bottom_left\"", WorkOrigin::StockBottomLeft),
    ] {
        let t = text.replace("\"drawing_origin\"", s);
        assert_eq!(Project::from_json(&t).unwrap().0.stock.origin, o);
    }
}

#[test]
fn v4_golden_has_the_picked_point() {
    let (pr, _) = Project::from_json(&fs::read_to_string(GOLDEN_V4).unwrap()).unwrap();
    assert_eq!(pr.stock.origin, WorkOrigin::Point(p(10.0, 5.0)));
    assert!(pr.items.iter().any(|i| i.layer == "Drawing"));
}

#[test]
fn too_new_v5_is_rejected_even_with_a_point_origin() {
    let t = with_origin("{\"point\": {\"x\": 1.0, \"y\": 2.0}}")
        .replace("\"format_version\": 4,", "\"format_version\": 5,");
    match Project::from_json(&t) {
        Err(e @ ProjectError::VersionTooNew { .. }) => {
            assert!(e.to_string().contains('5'), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn malformed_point_origin_is_a_clear_error() {
    for bad in [
        "{\"point\": {\"x\": 1.0}}",
        "{\"point\": {\"y\": 1.0}}",
        "{\"point\": {\"x\": \"1\", \"y\": 2.0}}",
        "{\"point\": {\"x\": NaN, \"y\": 2.0}}",
        "{\"point\": {\"x\": 1e999, \"y\": 2.0}}",
        "{\"point\": null}",
        "{\"point\": [1.0, 2.0, 3.0]}",
        "{\"points\": {\"x\": 1.0, \"y\": 2.0}}",
        "\"picked_point\"",
        "{}",
    ] {
        match Project::from_json(&with_origin(bad)) {
            Err(e @ ProjectError::Json(_)) => {
                let m = e.to_string();
                assert!(m.starts_with("invalid file content:"), "{bad}: {m}");
                // from_versioned_json goes through a serde_json::Value, so there is no line/column
                assert!(m.len() > "invalid file content: ".len() + 5, "{bad}: {m}");
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn non_finite_point_origin_cannot_be_saved() {
    for o in [p(f64::NAN, 0.0), p(0.0, f64::INFINITY)] {
        let mut pr = Project::new();
        pr.stock.origin = WorkOrigin::Point(o);
        assert!(
            matches!(pr.to_json(), Err(ProjectError::NonFinite)),
            "{o:?}"
        );
    }
}

#[test]
fn add_drawn_empty_and_existing_drawing_layer() {
    let mut pr = Project::new();
    assert!(pr.add_drawn(vec![]).is_empty());
    assert_eq!(pr.next_id, 1);
    // spec: the layer is added "if missing"; an empty call may add it, but only once
    assert!(pr.layers.len() <= 1);
    // a "Drawing" layer imported with another colour is reused, not duplicated or recoloured
    let mut pr = Project::new();
    pr.layers.push(hc_project::LayerInfo {
        name: "Drawing".into(),
        color: 3,
    });
    pr.add_drawn(vec![Geom::Point(p(0.0, 0.0))]);
    assert_eq!(pr.layers.len(), 1);
    assert_eq!(pr.layers[0].color, 3);
}

#[test]
fn transform_items_keeps_ops_and_tabs_and_ids() {
    let mut pr = Project::new();
    let ids = pr.add_drawn(vec![Geom::Contour(
        shapes::rect(p(0.0, 0.0), 10.0, 10.0, 2.0).unwrap(),
    )]);
    let t = hc_project::Tool {
        id: 1,
        name: "F".into(),
        kind: hc_project::ToolKind::FlatEndMill,
        diameter: 3.0,
        flute_length: 10.0,
        flutes: 2,
        feed: 500.0,
        plunge_feed: 100.0,
        rpm: 10000,
        depth_per_pass: 1.0,
        stepover_pct: 40.0,
        comment: String::new(),
    };
    pr.add_operation("e", OpKind::Engrave, &t, ids.clone());
    let ops = pr.operations.clone();
    pr.transform_items(&ids, &Transform::rotate(1.0, p(3.0, 4.0)));
    assert_eq!(pr.operations, ops);
    assert_eq!(pr.items[0].id, ids[0]);
    // empty id list: nothing changes
    let before = pr.clone();
    pr.transform_items(&[], &Transform::translate(1.0, 1.0));
    assert_eq!(pr, before);
}

#[test]
fn mirror_flips_every_arc_sweep_and_twice_is_identity() {
    let c = shapes::rect(p(0.0, 0.0), 30.0, 10.0, 3.0).unwrap();
    let mut pr = Project::new();
    let ids = pr.add_drawn(vec![Geom::Contour(c.clone())]);
    pr.transform_items(&ids, &Transform::mirror_x(5.0));
    let Geom::Contour(m) = &pr.items[0].geom else {
        panic!()
    };
    // the mirrored contour is connected and each arc's sweep sign flipped (CCW → CW)
    let n = m.segments.len();
    for i in 0..n {
        assert!(m.segments[i]
            .end()
            .approx_eq(m.segments[(i + 1) % n].start(), 1e-9));
    }
    for (a, b) in c.segments.iter().zip(&m.segments) {
        if let (Segment::Arc { sweep: sa, .. }, Segment::Arc { sweep: sb, .. }) = (a, b) {
            assert!((sa + sb).abs() < 1e-12, "{sa} {sb}");
        }
        // each arc's mid point mirrors too (it bulges the same physical way)
        let (ma, mb) = (a.midpoint(), b.midpoint());
        assert!(mb.approx_eq(p(ma.x, 10.0 - ma.y), 1e-9), "{ma:?} {mb:?}");
    }
    assert!((m.signed_area() + c.signed_area()).abs() < 1e-9);
    pr.transform_items(&ids, &Transform::mirror_x(5.0));
    let Geom::Contour(back) = &pr.items[0].geom else {
        panic!()
    };
    for (a, b) in c.segments.iter().zip(&back.segments) {
        assert!(a.start().approx_eq(b.start(), 1e-9) && a.end().approx_eq(b.end(), 1e-9));
        assert!(a.midpoint().approx_eq(b.midpoint(), 1e-9));
    }
}

#[test]
fn rotate_360_is_identity() {
    let c = shapes::rect(p(1.0, 2.0), 8.0, 4.0, 1.0).unwrap();
    let mut pr = Project::new();
    let ids = pr.add_drawn(vec![Geom::Contour(c.clone())]);
    pr.transform_items(&ids, &Transform::rotate(360f64.to_radians(), p(5.0, 4.0)));
    let Geom::Contour(r) = &pr.items[0].geom else {
        panic!()
    };
    for (a, b) in c.segments.iter().zip(&r.segments) {
        assert!(a.start().approx_eq(b.start(), 1e-9) && a.midpoint().approx_eq(b.midpoint(), 1e-9));
    }
    assert!((r.signed_area() - c.signed_area()).abs() < 1e-9);
}
