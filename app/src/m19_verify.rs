//! M19 acceptance tests: Measure tool (state level).

use crate::state::{
    fmt_angle, measure_angle, measure_summary, AppState, CanvasTool, DrawForm, Shape,
};
use hc_geom::Point;
use hc_project::Units;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn rect30x10() -> Shape {
    Shape::Rect {
        min: p(0.0, 0.0),
        w: 30.0,
        h: 10.0,
        r: 0.0,
    }
}

fn measure_state() -> AppState {
    let mut s = AppState::new(None);
    s.prefs.snap_grid = false; // M20: raw points
    s.set_tool(CanvasTool::Measure);
    s
}

/// S1 click sequence.
fn clicks(s: &mut AppState) {
    s.canvas_click(p(0.0, 0.0), false);
    s.canvas_click(p(30.0, 40.0), true);
    s.canvas_click(p(5.0, 5.0), false);
    s.canvas_click(p(5.0, 5.0), false);
}

fn fixed(s: &mut AppState) {
    s.measure = Some((p(0.0, 0.0), p(1.0, 1.0)));
}

#[test]
fn f1_angle() {
    let o = p(0.0, 0.0);
    for (b, want) in [
        (p(10.0, 0.0), 0.0),
        (p(0.0, 10.0), 90.0),
        (p(-10.0, 0.0), 180.0),
        (p(0.0, -10.0), 270.0),
        (p(10.0, -10.0), 315.0),
        (p(30.0, 40.0), 53.13010235415598),
    ] {
        let a = measure_angle(o, b).unwrap();
        assert!((a - want).abs() < 1e-9, "{b:?}: {a}");
    }
    assert_eq!(measure_angle(o, o), None);
    assert_eq!(measure_angle(o, p(1e-10, 0.0)), None);
}

#[test]
fn f2_fmt_angle() {
    assert_eq!(fmt_angle(Some(53.13010235)), "53.13°");
    assert_eq!(fmt_angle(Some(359.996)), "0.00°");
    assert_eq!(fmt_angle(Some(359.994)), "359.99°");
    assert_eq!(fmt_angle(Some(0.0)), "0.00°");
    assert_eq!(fmt_angle(None), "–");
}

#[test]
fn f3_f4_f5_summary() {
    let (o, m) = (p(0.0, 0.0), Units::Mm);
    assert_eq!(
        measure_summary(o, p(30.0, 40.0), m),
        "Distance 50.000 mm · ΔX 30.000 mm · ΔY 40.000 mm · Angle 53.13°"
    );
    assert_eq!(
        measure_summary(p(20.0, 20.0), p(50.0, 5.0), m),
        "Distance 33.541 mm · ΔX 30.000 mm · ΔY -15.000 mm · Angle 333.43°"
    );
    assert_eq!(
        measure_summary(o, p(25.4, 0.0), Units::Inch),
        "Distance 1.0000 in · ΔX 1.0000 in · ΔY 0.0000 in · Angle 0.00°"
    );
    assert_eq!(
        measure_summary(p(20.0, 20.0), p(50.0, 5.0), Units::Inch),
        "Distance 1.3205 in · ΔX 1.1811 in · ΔY -0.5906 in · Angle 333.43°"
    );
    assert_eq!(
        measure_summary(p(5.0, 5.0), p(5.0, 5.0), m),
        "Distance 0.000 mm · ΔX 0.000 mm · ΔY 0.000 mm · Angle –"
    );
}

#[test]
fn s1_click_flow() {
    let mut s = measure_state();
    s.canvas_click(p(0.0, 0.0), false);
    assert_eq!((s.pending, s.measure), (Some(p(0.0, 0.0)), None));
    s.canvas_click(p(30.0, 40.0), true);
    assert_eq!(s.pending, None);
    assert_eq!(s.measure, Some((p(0.0, 0.0), p(30.0, 40.0))));
    s.canvas_click(p(5.0, 5.0), false);
    assert_eq!((s.pending, s.measure), (Some(p(5.0, 5.0)), None));
    s.canvas_click(p(5.0, 5.0), false);
    assert_eq!(s.measure, Some((p(5.0, 5.0), p(5.0, 5.0))));
}

#[test]
fn s2_nothing_saved() {
    let mut s = AppState::new(None);
    s.add_shape(rect30x10()).unwrap();
    s.dirty = false;
    s.commit_undo(true);
    let (n, before, items) = (s.undo.len(), s.project.clone(), s.project.items.len());
    s.set_tool(CanvasTool::Measure);
    clicks(&mut s);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n);
    assert_eq!(s.project, before);
    assert_eq!(s.project, s.undo_base);
    assert!(!s.dirty);
    assert_eq!(s.project.items.len(), items);
}

#[test]
fn s3_escape() {
    let mut s = measure_state();
    fixed(&mut s);
    s.escape();
    assert_eq!((s.measure, s.tool), (None, CanvasTool::Measure));
    s.escape();
    assert_eq!(s.tool, CanvasTool::Select);
    let mut s = measure_state();
    s.canvas_click(p(1.0, 1.0), false);
    s.escape();
    assert_eq!((s.pending, s.tool), (None, CanvasTool::Measure));
}

#[test]
fn s4_cleared_by_tool_undo_new() {
    let mut s = measure_state();
    fixed(&mut s);
    s.set_tool(CanvasTool::Line);
    assert_eq!(s.measure, None);
    s.set_tool(CanvasTool::Measure);
    fixed(&mut s);
    s.set_tool(CanvasTool::Measure);
    assert_eq!(s.measure, None);
    s.add_shape(rect30x10()).unwrap();
    s.commit_undo(true);
    fixed(&mut s);
    assert!(s.undo());
    assert_eq!(s.measure, None);
    fixed(&mut s);
    s.new_project();
    assert_eq!(s.measure, None);
}

#[test]
fn s5_live() {
    let mut s = AppState::new(None);
    s.add_shape(rect30x10()).unwrap();
    s.view.zoom = 2.0;
    s.set_tool(CanvasTool::Measure);
    s.pending = Some(p(0.0, 20.0));
    s.cursor = Some(p(40.0, 30.0));
    assert_eq!(s.measure_live(), Some((p(0.0, 20.0), p(40.0, 30.0))));
    s.cursor = Some(p(30.3, 0.2));
    let (a, b) = s.measure_live().unwrap();
    assert!(a.approx_eq(p(0.0, 20.0), 1e-9) && b.approx_eq(p(30.0, 0.0), 1e-9));
    s.cursor = None;
    assert_eq!(s.measure_live(), None);
    s.cursor = Some(p(40.0, 30.0));
    s.tool = CanvasTool::Line;
    assert_eq!(s.measure_live(), None);
    s.tool = CanvasTool::Measure;
    s.pending = None;
    s.cursor = None;
    s.measure = Some((p(1.0, 1.0), p(2.0, 2.0)));
    assert_eq!(s.measure_live(), s.measure);
}

#[test]
fn s6_hint() {
    assert_eq!(
        CanvasTool::Measure.hint(false),
        Some("Measure: click the first point")
    );
    assert_eq!(
        CanvasTool::Measure.hint(true),
        Some("Measure: click the second point")
    );
    assert_eq!(DrawForm::default().shape(CanvasTool::Measure), None);
}

// ---------------------------------------------------------------- independent verification additions

use hc_project::Geom;

/// Snap-kind fixture: open L polyline-ish lines, a circle, and a point item, all far apart.
fn snap_state() -> AppState {
    let mut s = AppState::new(None);
    // Line (0,0)-(40,0): End, Midpoint (20,0). Line (20,-10)-(20,10): crosses it at (20,0) = Intersection too.
    s.prefs.snap_grid = false; // M20: raw points
    s.add_shape(Shape::Line {
        a: p(0.0, 0.0),
        b: p(40.0, 0.0),
    })
    .unwrap();
    s.add_shape(Shape::Line {
        a: p(20.0, -10.0),
        b: p(20.0, 10.0),
    })
    .unwrap();
    // Rect: Corner at (100,100); circle: Center (200,100)
    s.add_shape(Shape::Rect {
        min: p(100.0, 100.0),
        w: 30.0,
        h: 10.0,
        r: 0.0,
    })
    .unwrap();
    s.add_shape(Shape::Circle {
        c: p(200.0, 100.0),
        r: 5.0,
    })
    .unwrap();
    s.project.add_drawn(vec![Geom::Point(p(300.0, 100.0))]);
    s.items_changed();
    s.view.zoom = 2.0; // snap radius 5 mm
    s.set_tool(CanvasTool::Measure);
    s
}

#[test]
fn v_snap_kinds_for_a_and_b_live_and_raw() {
    use crate::snap::SnapKind::*;
    let s = snap_state();
    // (target, probe near it, kind)
    let cases = [
        (p(0.0, 0.0), End),
        (p(130.0, 110.0), Corner),
        (p(300.0, 100.0), Point),
        (p(200.0, 100.0), Center),
        (p(20.0, 0.0), Intersection), // also Midpoint at the same spot: Intersection sorts before Midpoint
        (p(20.0, 10.0), End),
        (p(40.0, 0.0), End),
    ];
    for (t, kind) in cases {
        let probe = p(t.x + 1.5, t.y - 1.5);
        let sn = s
            .snap_at(probe, 5.0)
            .unwrap_or_else(|| panic!("no snap near {t:?}"));
        assert!(sn.p.approx_eq(t, 1e-9), "{t:?} -> {:?}", sn.p);
        assert_eq!(sn.kind, kind, "{t:?}");
        // B live
        let mut m = snap_state();
        m.pending = Some(p(-50.0, -50.0));
        m.cursor = Some(probe);
        let (_, b) = m.measure_live().unwrap();
        assert!(b.approx_eq(t, 1e-9));
    }
    // Midpoint alone (rect top edge midpoint (115,110) )
    let sn = s.snap_at(p(116.0, 109.0), 5.0).unwrap();
    assert_eq!(sn.kind, Midpoint);
    assert!(sn.p.approx_eq(p(115.0, 110.0), 1e-9));
    // raw when nothing near
    let mut m = snap_state();
    m.pending = Some(p(0.0, 0.0));
    m.cursor = Some(p(77.0, 33.0));
    assert_eq!(m.measure_live(), Some((p(0.0, 0.0), p(77.0, 33.0))));
    // click flow uses whatever point it is given: raw A
    let mut m = snap_state();
    m.canvas_click(p(77.0, 33.0), false);
    assert_eq!(m.pending, Some(p(77.0, 33.0)));
}

#[test]
fn v_hidden_layer_snaps_ignored() {
    let mut s = snap_state();
    let layer = s.project.items[2].layer.clone(); // rect and others share the drawn layer
    s.set_layer_visible(&layer, false);
    assert!(s.snap_at(p(100.5, 100.5), 5.0).is_none());
    s.pending = Some(p(0.0, 0.0));
    s.cursor = Some(p(100.5, 100.5));
    assert_eq!(s.measure_live(), Some((p(0.0, 0.0), p(100.5, 100.5))));
    s.set_layer_visible(&layer, true);
    let (_, b) = s.measure_live().unwrap();
    assert!(b.approx_eq(p(100.0, 100.0), 1e-9));
}

#[test]
fn v_live_follows_cursor_and_clears_off_canvas() {
    let mut s = measure_state();
    s.canvas_click(p(0.0, 0.0), false);
    for c in [p(10.0, 0.0), p(10.0, 5.0), p(-3.0, 8.0)] {
        s.cursor = Some(c);
        assert_eq!(s.measure_live(), Some((p(0.0, 0.0), c)));
    }
    s.cursor = None;
    assert_eq!(s.measure_live(), None);
    assert_eq!(s.pending, Some(p(0.0, 0.0))); // A kept
    s.cursor = Some(p(1.0, 1.0));
    assert!(s.measure_live().is_some());
    // no A yet -> nothing live
    let mut s = measure_state();
    s.cursor = Some(p(1.0, 1.0));
    assert_eq!(s.measure_live(), None);
}

#[test]
fn v_signs_four_quadrants_and_angles() {
    let o = p(10.0, 10.0);
    let m = Units::Mm;
    for (b, dx, dy, ang) in [
        (p(13.0, 14.0), "3.000", "4.000", "53.13°"),
        (p(7.0, 14.0), "-3.000", "4.000", "126.87°"),
        (p(7.0, 6.0), "-3.000", "-4.000", "233.13°"),
        (p(13.0, 6.0), "3.000", "-4.000", "306.87°"),
    ] {
        let t = measure_summary(o, b, m);
        assert!(t.contains(&format!("ΔX {dx} mm")), "{t}");
        assert!(t.contains(&format!("ΔY {dy} mm")), "{t}");
        assert!(t.ends_with(&format!("Angle {ang}")), "{t}");
        assert!(t.starts_with("Distance 5.000 mm"), "{t}");
    }
    for (b, want) in [
        (p(1.0, 0.0), "0.00°"),
        (p(0.0, 1.0), "90.00°"),
        (p(-1.0, 0.0), "180.00°"),
        (p(0.0, -1.0), "270.00°"),
    ] {
        assert_eq!(fmt_angle(measure_angle(p(0.0, 0.0), b)), want);
    }
}

#[test]
fn v_angle_just_below_360_and_range() {
    // -0.001 degrees
    let b = p(
        (-0.001f64).to_radians().cos() * 100.0,
        (-0.001f64).to_radians().sin() * 100.0,
    );
    let a = measure_angle(p(0.0, 0.0), b).unwrap();
    assert!((359.99..360.0).contains(&a), "{a}");
    assert_eq!(fmt_angle(Some(a)), "0.00°"); // spec: reads 0.00
                                             // tiny negative dy must still be in [0, 360)
    for dy in [-1e-15, -1e-300, -0.0] {
        let a = measure_angle(p(0.0, 0.0), p(100.0, dy)).unwrap();
        assert!((0.0..360.0).contains(&a), "dy={dy}: {a}");
    }
    assert_eq!(
        fmt_angle(measure_angle(p(0.0, 0.0), p(100.0, -1e-15))),
        "0.00°"
    );
}

#[test]
fn v_zero_threshold_boundary() {
    assert!(measure_angle(p(0.0, 0.0), p(1e-9, 0.0)).is_some());
    assert!(measure_angle(p(0.0, 0.0), p(0.9e-9, 0.0)).is_none());
    // tiny negative delta prints -0.000 (accepted by spec)
    assert!(measure_summary(p(0.0, 0.0), p(-1e-7, 0.0), Units::Mm).contains("ΔX -0.000 mm"));
}

#[test]
fn v_extreme_coordinates() {
    let t = measure_summary(p(-1e6, -1e6), p(1e6, 1e6), Units::Mm);
    assert!(t.starts_with("Distance 2828427.125 mm"), "{t}");
    assert!(t.ends_with("Angle 45.00°"), "{t}");
    let t = measure_summary(p(0.0, 0.0), p(0.001, 0.0), Units::Mm);
    assert_eq!(
        t,
        "Distance 0.001 mm · ΔX 0.001 mm · ΔY 0.000 mm · Angle 0.00°"
    );
    let t = measure_summary(p(0.0, 0.0), p(0.0, 0.001), Units::Inch);
    assert!(
        t.contains("Distance 0.0000 in") && t.ends_with("Angle 90.00°"),
        "{t}"
    );
    // offset far from origin keeps the delta
    let t = measure_summary(p(1e7, 1e7), p(1e7 + 3.0, 1e7 + 4.0), Units::Mm);
    assert!(t.starts_with("Distance 5.000 mm"), "{t}");
}

#[test]
fn v_units_switch_while_shown() {
    let mut s = measure_state();
    s.measure = Some((p(0.0, 0.0), p(25.4, 0.0)));
    let (a, b) = s.measure_live().unwrap();
    assert!(measure_summary(a, b, s.project.units).starts_with("Distance 25.400 mm"));
    s.set_units(Units::Inch);
    assert_eq!(s.measure, Some((a, b))); // set_units keeps it
    let (a, b) = s.measure_live().unwrap();
    assert!(measure_summary(a, b, s.project.units).starts_with("Distance 1.0000 in"));
}

#[test]
fn v_escape_order_and_set_tool_pending() {
    let mut s = measure_state();
    s.canvas_click(p(1.0, 1.0), false);
    s.set_tool(CanvasTool::Line);
    assert_eq!(s.pending, None);
    s.set_tool(CanvasTool::Measure);
    s.canvas_click(p(1.0, 1.0), false);
    s.canvas_click(p(2.0, 2.0), false);
    s.canvas_click(p(3.0, 3.0), false); // new A clears the fixed one
    assert_eq!(s.measure, None);
    s.escape();
    assert_eq!(
        (s.pending, s.measure, s.tool),
        (None, None, CanvasTool::Measure)
    );
    s.escape();
    assert_eq!(s.tool, CanvasTool::Select);
    s.escape(); // no panic, no tool change
    assert_eq!(s.tool, CanvasTool::Select);
}

#[test]
fn v_redo_and_open_clear() {
    let mut s = measure_state();
    s.add_shape(rect30x10()).unwrap();
    s.commit_undo(true);
    assert!(s.undo());
    s.set_tool(CanvasTool::Measure);
    fixed(&mut s);
    assert!(s.redo());
    assert_eq!(s.measure, None);
    s.canvas_click(p(1.0, 1.0), false);
    assert!(s.undo());
    assert_eq!(s.pending, None);
}

#[test]
fn v_measure_with_operations_leaves_toolpaths_alone() {
    let (mut s, op) = crate::state::tests::bracket_clean();
    s.dirty = false;
    s.commit_undo(true);
    let (n, before, tps) = (s.undo.len(), s.project.clone(), s.toolpaths.len());
    s.set_tool(CanvasTool::Measure);
    clicks(&mut s);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n);
    assert_eq!(s.project, before);
    assert_eq!(s.toolpaths.len(), tps);
    assert!(s.project.operations.iter().any(|o| o.id == op));
    assert!(!s.dirty);
}
