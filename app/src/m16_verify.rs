//! M16 acceptance tests: Round corners (state level).

use crate::state::tests::{edit_op, op_on, tool};
use crate::state::{AppState, CanvasTool, NewKind, Severity, Shape};
use hc_geom::{Contour, Point, Segment};
use hc_project::{Geom, ManualTabs};
use std::collections::BTreeSet;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn contour(s: &AppState, id: u64) -> Contour {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.clone(),
        Geom::Point(_) => panic!("point"),
    }
}

/// Sharp 30 x 10 rect at the origin, Fillet tool, radius 2, clean undo state.
fn setup() -> (AppState, u64) {
    let mut s = AppState::new(None);
    let id = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    s.set_tool(CanvasTool::Fillet);
    s.draw.fillet_r = 2.0;
    s.commit_undo(true);
    s.dirty = false;
    (s, id)
}

fn last(s: &AppState) -> (Severity, &str) {
    let m = s.messages.last().unwrap();
    (m.severity, &m.text)
}

fn d_shape() -> Contour {
    Contour {
        segments: vec![
            Segment::Line {
                start: p(0.0, 0.0),
                end: p(10.0, 0.0),
            },
            Segment::Arc {
                center: p(5.0, 0.0),
                radius: 5.0,
                start_angle: 0.0,
                sweep: std::f64::consts::PI,
            },
        ],
        closed: true,
    }
}

#[test]
fn s1_click_rounds_one_undo_step() {
    let (mut s, id) = setup();
    let n = s.undo.len();
    s.fillet_click(p(30.2, 0.1), 0.5);
    assert_eq!(contour(&s, id).segments.len(), 5);
    assert!(s.dirty);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
    s.undo();
    assert_eq!(contour(&s, id).segments.len(), 4);
}

#[test]
fn s2_miss_does_nothing() {
    let (mut s, _) = setup();
    let before = s.project.clone();
    let m = s.messages.len();
    s.fillet_click(p(15.0, 5.0), 0.5);
    assert_eq!(s.project, before);
    assert_eq!(s.messages.len(), m);
    assert!(!s.dirty);
}

#[test]
fn s3_all_then_recognized_as_rect() {
    let (mut s, id) = setup();
    s.selection = BTreeSet::from([id]);
    let n = s.undo.len();
    s.fillet_selection();
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
    match s.item_shape(id) {
        Some(Shape::Rect { min, w, h, r }) => {
            assert!(min.approx_eq(p(0.0, 0.0), 1e-6));
            assert!((w - 30.0).abs() < 1e-6 && (h - 10.0).abs() < 1e-6 && (r - 2.0).abs() < 1e-6);
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(last(&s), (Severity::Info, "Rounded 4 corners"));
}

#[test]
fn s4_skipped_too_large() {
    let (mut s, id) = setup();
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 6.0;
    s.fillet_selection();
    assert_eq!(
        last(&s),
        (
            Severity::Warning,
            "Rounded 2 corners; skipped 2 (radius too large: 2)"
        )
    );
}

#[test]
fn s5_mixed_selection() {
    let (mut s, rect) = setup();
    let ids = s
        .project
        .add_drawn(vec![Geom::Contour(d_shape()), Geom::Point(p(50.0, 50.0))]);
    s.items_changed();
    s.selection = BTreeSet::from([rect, ids[0], ids[1]]);
    s.fillet_selection();
    assert_eq!(
        last(&s),
        (
            Severity::Warning,
            "Rounded 4 corners; skipped 2 (not between two straight lines: 2)"
        )
    );
    assert_eq!(contour(&s, ids[0]), d_shape());
    assert_eq!(s.item(ids[1]).unwrap().geom, Geom::Point(p(50.0, 50.0)));
}

#[test]
fn s6_nothing_to_round() {
    let mut s = AppState::new(None);
    let c = s
        .add_shape(Shape::Circle {
            c: p(0.0, 0.0),
            r: 5.0,
        })
        .unwrap()[0];
    s.commit_undo(true);
    s.dirty = false;
    let n = s.undo.len();
    s.selection = BTreeSet::from([c]);
    s.fillet_selection();
    s.commit_undo(true);
    assert_eq!(
        last(&s),
        (Severity::Info, "No sharp corners in the selection")
    );
    assert!(!s.dirty);
    assert_eq!(s.undo.len(), n);
}

#[test]
fn s7_bad_radius() {
    let (mut s, id) = setup();
    let before = s.project.clone();
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 0.0;
    s.fillet_selection();
    assert_eq!(
        last(&s),
        (Severity::Warning, "Radius must be greater than 0")
    );
    s.messages.clear();
    s.fillet_click(p(30.0, 0.0), 0.5);
    assert_eq!(
        last(&s),
        (Severity::Warning, "Radius must be greater than 0")
    );
    assert_eq!(s.project, before);
}

#[test]
fn s8_click_skip_messages() {
    let (mut s, _) = setup();
    s.draw.fillet_r = 11.0;
    s.fillet_click(p(30.0, 0.0), 0.5);
    assert_eq!(
        last(&s),
        (
            Severity::Warning,
            "Corner skipped: radius too large (at most 10.000 mm)"
        )
    );
    let rr = s
        .add_shape(Shape::Rect {
            min: p(100.0, 100.0),
            w: 30.0,
            h: 10.0,
            r: 2.0,
        })
        .unwrap()[0];
    s.draw.fillet_r = 1.0;
    s.fillet_click(p(128.0, 100.0), 0.5); // line / arc tangent joint
    assert_eq!(last(&s), (Severity::Warning, "Not a sharp corner"));
    assert_eq!(contour(&s, rr).segments.len(), 8);
    s.project.add_drawn(vec![Geom::Contour(d_shape())]);
    s.items_changed();
    s.fillet_click(p(10.0, 0.0), 0.1);
    // (10,0) is shared with the rect's neighbourhood? no: the rect corner is (30,0), so the D corner is the hit
    assert_eq!(
        last(&s),
        (
            Severity::Warning,
            "Corner skipped: not between two straight lines"
        )
    );
}

#[test]
fn s9_tabs_follow() {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let id = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let op = op_on(&mut s, vec![id], NewKind::Contour, 1);
    edit_op(&mut s, op, |o| {
        o.depth = 5.0;
        o.tabs.enabled = true;
        o.tabs.manual = vec![ManualTabs {
            item: id,
            positions: vec![0.25],
        }];
    });
    assert!(contour(&s, id)
        .point_at(0.25 * 80.0)
        .approx_eq(p(20.0, 0.0), 1e-9));
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 2.0;
    s.fillet_selection();
    let o = s.project.operations.iter().find(|o| o.id == op).unwrap();
    assert_eq!(o.items, vec![id]);
    let t = o.tabs.manual[0].positions[0];
    let new = contour(&s, id);
    assert!(new.point_at(t * new.length()).approx_eq(p(20.0, 0.0), 1e-9));
    s.refresh_toolpaths();
    assert!(matches!(s.toolpath_result(op), Some(Ok(_))));
}

#[test]
fn s10_hidden_layer() {
    let (mut s, id) = setup();
    assert!(s.fillet_hover(p(30.0, 0.0), 0.5).is_some());
    let layer = s.item(id).unwrap().layer.clone();
    s.set_layer_visible(&layer, false);
    assert!(s.fillet_hover(p(30.0, 0.0), 0.5).is_none());
}

#[test]
fn s11_escape_and_hint() {
    let (mut s, id) = setup();
    s.selection = BTreeSet::from([id]);
    s.escape();
    assert_eq!(s.tool, CanvasTool::Select);
    assert_eq!(s.selection, BTreeSet::from([id]));
    assert_eq!(
        CanvasTool::Fillet.hint(false),
        Some("Round corners: click a sharp corner")
    );
}

#[test]
fn s12_hover() {
    let (s, id) = setup();
    let h = s.fillet_hover(p(29.9, 0.1), 0.5).unwrap();
    assert_eq!((h.item, h.joint), (id, 1));
    assert!(h.vertex.approx_eq(p(30.0, 0.0), 1e-9));
    match h.result {
        Ok(Segment::Arc { center, .. }) => assert!(center.approx_eq(p(28.0, 2.0), 1e-9)),
        o => panic!("{o:?}"),
    }
}

// ---- verifier additions ----

use crate::state::tests::{bracket_path, outer};
use hc_geom::fillet;
use hc_project::Units;

#[test]
fn v_click_on_empty_canvas_and_empty_project() {
    let mut s = AppState::new(None);
    s.set_tool(CanvasTool::Fillet);
    let m = s.messages.len();
    s.fillet_click(p(5.0, 5.0), 1.0);
    s.fillet_selection();
    assert_eq!(s.messages.len(), m);
    assert!(s.fillet_hover(p(0.0, 0.0), 1.0).is_none());
}

#[test]
fn v_hover_on_non_corner_and_tangent_joint() {
    let (mut s, _) = setup();
    assert!(s.fillet_hover(p(15.0, 0.0), 0.5).is_none()); // mid edge
    assert!(s.fillet_hover(p(15.0, 5.0), 0.5).is_none()); // interior
    s.add_shape(Shape::Rect {
        min: p(100.0, 100.0),
        w: 30.0,
        h: 10.0,
        r: 2.0,
    })
    .unwrap();
    // tangent joint is reported as a hover with NotSharp (label "Not sharp")
    let h = s.fillet_hover(p(128.0, 100.0), 0.2).unwrap();
    assert_eq!(h.result, Err(fillet::FilletSkip::NotSharp));
}

#[test]
fn v_bad_radius_values() {
    for r in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-12] {
        let (mut s, id) = setup();
        let before = s.project.clone();
        s.draw.fillet_r = r;
        s.selection = BTreeSet::from([id]);
        s.fillet_selection();
        assert_eq!(
            last(&s),
            (Severity::Warning, "Radius must be greater than 0"),
            "r={r}"
        );
        s.messages.clear();
        s.fillet_click(p(30.0, 0.0), 0.5);
        assert_eq!(
            last(&s),
            (Severity::Warning, "Radius must be greater than 0"),
            "r={r}"
        );
        assert_eq!(s.project, before);
        assert!(!s.dirty);
    }
}

#[test]
fn v_inch_message() {
    let (mut s, _) = setup();
    s.set_units(Units::Inch);
    s.draw.fillet_r = 11.0;
    s.fillet_click(p(30.0, 0.0), 0.5);
    let (sev, txt) = last(&s);
    assert_eq!(sev, Severity::Warning);
    assert!(
        txt.starts_with("Corner skipped: radius too large (at most 0.39") && txt.ends_with("in)"),
        "{txt}"
    );
}

#[test]
fn v_one_undo_step_per_click_and_button_and_redo() {
    let (mut s, id) = setup();
    let n = s.undo.len();
    s.fillet_click(p(30.0, 0.0), 0.5);
    s.commit_undo(true);
    s.fillet_click(p(30.0, 10.0), 0.5);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 2);
    s.selection = BTreeSet::from([id]);
    s.fillet_selection();
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 3);
    assert_eq!(contour(&s, id).segments.len(), 8);
    s.undo();
    assert_eq!(contour(&s, id).segments.len(), 6);
    s.undo();
    s.undo();
    assert_eq!(contour(&s, id).segments.len(), 4);
    s.redo();
    assert_eq!(contour(&s, id).segments.len(), 5);
}

#[test]
fn v_second_click_on_same_corner_is_not_sharp() {
    let (mut s, id) = setup();
    s.fillet_click(p(30.0, 0.0), 0.3);
    assert_eq!(contour(&s, id).segments.len(), 5);
    // the corner vertex is gone; nearest joints are the tangent ones at (28,0)/(30,2), 2.8 away
    s.fillet_click(p(30.0, 0.0), 0.3);
    assert_eq!(contour(&s, id).segments.len(), 5);
}

#[test]
fn v_click_each_corner_matches_fillet_all() {
    let (mut s, id) = setup();
    for c in [(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (0.0, 10.0)] {
        s.fillet_click(p(c.0, c.1), 0.3);
    }
    let a = contour(&s, id);
    assert_eq!(a.segments.len(), 8);
    assert!(matches!(
        s.item_shape(id),
        Some(Shape::Rect { r, .. }) if (r - 2.0).abs() < 1e-6
    ));
    assert!(a.signed_area() > 0.0);
}

#[test]
fn v_item_identity_and_ops_survive() {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let id = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let op = op_on(&mut s, vec![id], NewKind::Contour, 1);
    edit_op(&mut s, op, |o| o.depth = 2.0);
    s.refresh_toolpaths();
    let before = match s.toolpath_result(op) {
        Some(Ok(t)) => format!("{:?}", t.moves),
        o => panic!("{o:?}"),
    };
    let layer = s.item(id).unwrap().layer.clone();
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 3.0;
    s.fillet_selection();
    s.refresh_toolpaths();
    assert_eq!(s.item(id).unwrap().layer, layer);
    assert_eq!(s.project.items.len(), 1);
    assert_eq!(s.project.operations.len(), 1);
    assert_eq!(s.project.operations[0].items, vec![id]);
    let after = match s.toolpath_result(op) {
        Some(Ok(t)) => format!("{:?}", t.moves),
        o => panic!("{o:?}"),
    };
    assert_ne!(
        before, after,
        "toolpath must be regenerated for the new shape"
    );
}

#[test]
fn v_tab_on_cut_corner_lands_on_arc_and_stays_in_range() {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let id = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let op = op_on(&mut s, vec![id], NewKind::Contour, 1);
    edit_op(&mut s, op, |o| {
        o.depth = 5.0;
        o.tabs.enabled = true;
        // 0.0 = on joint 0 corner (0,0); 0.375 = (30,0) corner; 0.999 near the start; 1.25 wraps
        o.tabs.manual = vec![ManualTabs {
            item: id,
            positions: vec![0.0, 0.375, 0.999, 1.25],
        }];
    });
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 2.0;
    s.fillet_selection();
    let new = contour(&s, id);
    let o = s.project.operations.iter().find(|o| o.id == op).unwrap();
    for &t in &o.tabs.manual[0].positions {
        assert!((0.0..1.0).contains(&t), "{t}");
        let pt = new.point_at(t * new.length());
        assert!(new.closest(pt).1 < 1e-9);
    }
    // corner tab at (30,0) maps onto the new arc, within r of the original corner
    let pt = new.point_at(o.tabs.manual[0].positions[1] * new.length());
    assert!(pt.distance(p(30.0, 0.0)) < 1.0, "{pt:?}");
    assert!(pt.distance(p(28.0, 2.0)) - 2.0 < 1e-9);
    s.refresh_toolpaths();
    assert!(matches!(s.toolpath_result(op), Some(Ok(_))));
}

#[test]
fn v_tabs_of_other_items_untouched() {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let a = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let b = s
        .add_shape(Shape::Rect {
            min: p(50.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let op = op_on(&mut s, vec![a, b], NewKind::Contour, 1);
    edit_op(&mut s, op, |o| {
        o.depth = 5.0;
        o.tabs.enabled = true;
        o.tabs.manual = vec![
            ManualTabs {
                item: a,
                positions: vec![0.25],
            },
            ManualTabs {
                item: b,
                positions: vec![0.25],
            },
        ];
    });
    s.selection = BTreeSet::from([a]);
    s.draw.fillet_r = 2.0;
    s.fillet_selection();
    let o = s.project.operations.iter().find(|o| o.id == op).unwrap();
    assert_eq!(o.tabs.manual[1].positions, vec![0.25]);
    assert_ne!(o.tabs.manual[0].positions, vec![0.25]);
}

#[test]
fn v_hidden_layer_item_ignored_by_click() {
    let (mut s, id) = setup();
    let layer = s.item(id).unwrap().layer.clone();
    s.set_layer_visible(&layer, false);
    let before = s.project.clone();
    s.fillet_click(p(30.0, 0.0), 0.5);
    assert_eq!(s.project, before);
}

#[test]
fn v_overlapping_items_nearest_joint_wins_first_on_tie() {
    let (mut s, a) = setup();
    let b = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    assert_eq!(s.fillet_hover(p(30.0, 0.0), 0.5).unwrap().item, a);
    s.fillet_click(p(30.0, 0.0), 0.5);
    assert_eq!(contour(&s, a).segments.len(), 5);
    assert_eq!(contour(&s, b).segments.len(), 4);
}

#[test]
fn v_selection_with_contour_and_no_other_changes() {
    let (mut s, id) = setup();
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 2.0;
    s.fillet_selection();
    s.messages.clear();
    s.fillet_selection(); // already round: nothing sharp
    assert_eq!(
        last(&s),
        (Severity::Info, "No sharp corners in the selection")
    );
}

#[test]
fn v_single_corner_message_is_singular() {
    let (mut s, id) = setup();
    // open polyline with a single joint
    let open = Contour {
        segments: vec![
            Segment::Line {
                start: p(0.0, 20.0),
                end: p(10.0, 20.0),
            },
            Segment::Line {
                start: p(10.0, 20.0),
                end: p(10.0, 30.0),
            },
        ],
        closed: false,
    };
    let o = s.project.add_drawn(vec![Geom::Contour(open)]);
    s.items_changed();
    s.selection = BTreeSet::from([o[0]]);
    let _ = id;
    s.fillet_selection();
    assert_eq!(last(&s), (Severity::Info, "Rounded 1 corner"));
}

#[test]
fn v_bracket_outer_contour() {
    let mut s = AppState::new(None);
    s.open_cli(&bracket_path());
    let id = outer(&s);
    let c = contour(&s, id);
    let before_len = c.length();
    let arcs = c
        .segments
        .iter()
        .filter(|g| matches!(g, Segment::Arc { .. }))
        .count();
    let f = fillet::fillet_all(&c, 2.0);
    eprintln!(
        "bracket outer: {} segs, {arcs} arcs; rounded {} not_lines {} too_large {}",
        c.segments.len(),
        f.rounded,
        f.not_lines,
        f.too_large
    );
    for i in 0..f.contour.segments.len() {
        let e = f.contour.segments[i].end();
        let st = f.contour.segments[(i + 1) % f.contour.segments.len()].start();
        assert!(e.approx_eq(st, 1e-6), "gap {i}");
    }
    assert!(f.contour.length() <= before_len + 1e-9);
    assert_eq!(f.contour.orientation(), c.orientation());
    if arcs > 0 {
        // every sharp joint adjacent to an arc is reported, none silently dropped
        let sharp_with_arc = fillet::joints(&c)
            .iter()
            .filter(|(k, _)| {
                let n = c.segments.len();
                let (a, b) = (&c.segments[(k + n - 1) % n], &c.segments[*k]);
                matches!(a, Segment::Arc { .. }) || matches!(b, Segment::Arc { .. })
            })
            .count();
        assert!(f.not_lines <= sharp_with_arc);
    }
    // via the app: one message, undoable, toolpath for an op on it still generates
    s.library.add(tool(1)).unwrap();
    let op = op_on(&mut s, vec![id], NewKind::Contour, 1);
    edit_op(&mut s, op, |o| o.depth = 3.0);
    s.commit_undo(true);
    let n = s.undo.len();
    s.selection = BTreeSet::from([id]);
    s.draw.fillet_r = 2.0;
    s.fillet_selection();
    s.commit_undo(true);
    s.refresh_toolpaths();
    assert!(matches!(s.toolpath_result(op), Some(Ok(_))));
    assert!(s.undo.len() <= n + 1);
}

#[test]
fn v_offset_and_pocket_on_filleted_contours() {
    use hc_toolpath::{offset_closed, pocket_rings};
    let sharp = hc_geom::shapes::rect(p(0.0, 0.0), 30.0, 10.0, 0.0).unwrap();
    for r in [1e-4, 0.5, 2.0, 4.9, 5.0] {
        let f = fillet::fillet_all(&sharp, r).contour;
        for d in [-1.0, -0.5, 0.5, 2.0] {
            let _ = offset_closed(&f, d);
        }
        let rings = pocket_rings(&f, &[], 1.5, 1.0);
        assert!(!rings.is_empty(), "r={r}");
    }
    // concave L-shape
    let l = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(20.0, 0.0), 0.0),
            (p(20.0, 10.0), 0.0),
            (p(10.0, 10.0), 0.0),
            (p(10.0, 20.0), 0.0),
            (p(0.0, 20.0), 0.0),
        ],
        true,
    );
    let f = fillet::fillet_all(&l, 2.0).contour;
    assert!(!offset_closed(&f, -1.0).is_empty());
    assert!(!pocket_rings(&f, &[], 1.5, 1.0).is_empty());
}
