//! M10a verifier tests (state + kittest): snap, drawing, transforms, delete, undo/redo, origin.

use crate::snap::{self, SnapKind, SNAP_PX};
use crate::state::tests::{bracket_clean, bracket_path, edit_op, outer, scratch, tool};
use crate::state::{AppState, CanvasTool, NewKind, Severity, Shape, UNDO_DEPTH};
use hc_geom::{Point, Transform};
use hc_project::{Geom, ManualTabs, Units, WorkOrigin};
use hc_toolpath::{Move, RuleId};

const T: f64 = 1e-9;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn rect(s: &mut AppState, x: f64, y: f64, w: f64, h: f64, r: f64) -> u64 {
    s.add_shape(Shape::Rect {
        min: p(x, y),
        w,
        h,
        r,
    })
    .unwrap()[0]
}

fn contour(s: &AppState, id: u64) -> hc_geom::Contour {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.clone(),
        Geom::Point(_) => panic!("point"),
    }
}

/// Signed area of the XY feed positions below Z0 (sign = travel direction of the cut).
fn cut_area(s: &AppState, op: u64) -> f64 {
    let tp = s.toolpath_result(op).unwrap().as_ref().unwrap();
    let pts: Vec<Point> = tp
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Linear { to, .. } | Move::Arc { to, .. } if to.z < -1e-6 => Some(to.xy()),
            _ => None,
        })
        .collect();
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

// ---------------------------------------------------------------- snap

#[test]
fn snap_hidden_layer_excluded_even_when_nearer() {
    let mut s = AppState::new(None);
    rect(&mut s, 0.0, 0.0, 10.0, 10.0, 0.0);
    // a drill point on another layer right at the cursor
    s.project.layers.push(hc_project::LayerInfo {
        name: "Holes".into(),
        color: 1,
    });
    let id = s.project.add_drawn(vec![Geom::Point(p(0.5, 0.0))])[0];
    s.project
        .items
        .iter_mut()
        .find(|i| i.id == id)
        .unwrap()
        .layer = "Holes".into();
    // any state edit runs items_changed
    s.add_shape(Shape::Line {
        a: p(90.0, 90.0),
        b: p(99.0, 90.0),
    })
    .unwrap();
    assert_eq!(s.snap_at(p(0.5, 0.0), 1.0).unwrap().kind, SnapKind::Point);
    s.set_layer_visible("Holes", false);
    let sn = s.snap_at(p(0.5, 0.0), 1.0).unwrap();
    assert_eq!((sn.kind, sn.p), (SnapKind::Corner, p(0.0, 0.0)));
}

#[test]
fn snap_ties_across_items_follow_the_kind_order() {
    // End (line start) = Corner (rect) = Point (drill) = Center (circle) at (5,5)
    let mut s = AppState::new(None);
    s.add_shape(Shape::Circle {
        c: p(5.0, 5.0),
        r: 2.0,
    })
    .unwrap();
    s.project.add_drawn(vec![Geom::Point(p(5.0, 5.0))]);
    rect(&mut s, 5.0, 5.0, 3.0, 3.0, 0.0);
    let at = p(5.3, 5.2);
    assert_eq!(s.snap_at(at, 1.0).unwrap().kind, SnapKind::Corner);
    s.add_shape(Shape::Line {
        a: p(5.0, 5.0),
        b: p(0.0, 9.0),
    })
    .unwrap();
    assert_eq!(s.snap_at(at, 1.0).unwrap().kind, SnapKind::End);
    // only Point and Center left
    let mut s = AppState::new(None);
    s.add_shape(Shape::Circle {
        c: p(5.0, 5.0),
        r: 2.0,
    })
    .unwrap();
    s.project.add_drawn(vec![Geom::Point(p(5.0, 5.0))]);
    s.add_shape(Shape::Line {
        a: p(50.0, 50.0),
        b: p(60.0, 50.0),
    })
    .unwrap(); // any items_changed refreshes the cache
    let sn = s.snap_at(at, 1.0).unwrap();
    assert_eq!((sn.kind, sn.p), (SnapKind::Point, p(5.0, 5.0)));
    // a strictly nearer Center beats a farther End
    let sn = s.snap_at(p(5.0, 5.0), 100.0).unwrap();
    assert_eq!(sn.p, p(5.0, 5.0));
}

#[test]
fn snap_rounded_rect_arc_centres_and_radius_boundary() {
    let mut s = AppState::new(None);
    rect(&mut s, 0.0, 0.0, 20.0, 10.0, 3.0);
    let sn = s.snap_at(p(3.2, 3.1), 1.0).unwrap();
    assert_eq!((sn.kind, sn.p), (SnapKind::Center, p(3.0, 3.0)));
    // radius boundary: exactly at the radius snaps, just beyond does not
    let r = SNAP_PX / 4.0;
    assert!(s.snap_at(p(3.0 + r, 3.0), r).is_some());
    assert!(s.snap_at(p(3.0 + r + 1e-6, 3.0), r).is_none());
}

#[test]
fn snap_cache_follows_transform_delete_undo_and_import() {
    let mut s = AppState::new(None);
    let id = rect(&mut s, 0.0, 0.0, 10.0, 10.0, 0.0);
    s.commit_undo(true);
    s.transform_selection(&Transform::translate(100.0, 0.0));
    s.commit_undo(true);
    assert!(s.snap_at(p(0.0, 0.0), 0.5).is_none());
    assert_eq!(s.snap_at(p(100.0, 0.0), 0.5).unwrap().item, id);
    s.selection = [id].into_iter().collect();
    s.delete_selection();
    s.commit_undo(true);
    assert!(s.snap_at(p(100.0, 0.0), 0.5).is_none());
    assert!(s.snap_cache.cands.is_empty()); // M10a.2: SnapCache
    s.undo();
    assert_eq!(s.snap_at(p(100.0, 0.0), 0.5).unwrap().item, id);
    s.undo();
    assert_eq!(s.snap_at(p(0.0, 0.0), 0.5).unwrap().item, id);
    assert!(s.snap_at(p(100.0, 0.0), 0.5).is_none());
    s.redo();
    assert!(s.snap_at(p(100.0, 0.0), 0.5).is_some());
    assert!(s.import_dxf(&bracket_path()));
    assert_eq!(s.snap_cache, snap::build_cache(&s.project.items));
}

#[test]
fn snap_cache_matches_after_every_mutation_kind() {
    let (mut s, outside) = bracket_clean();
    s.commit_undo(true);
    let check = |s: &AppState, what: &str| {
        assert_eq!(s.snap_cache, snap::build_cache(&s.project.items), "{what}");
        assert_eq!(s.item_bbox.len(), s.project.items.len(), "{what}");
    };
    check(&s, "open");
    s.selection = [outer(&s)].into_iter().collect();
    s.transform_selection(&Transform::rotate(0.3, p(0.0, 0.0)));
    check(&s, "rotate");
    s.delete_selection();
    check(&s, "delete");
    s.commit_undo(true);
    s.undo();
    check(&s, "undo");
    edit_op(&mut s, outside, |o| o.depth = 2.0);
    check(&s, "op edit");
    s.new_project();
    check(&s, "new");
}

// ---------------------------------------------------------------- drawing

#[test]
fn drawing_degenerate_and_negative_sizes_rejected() {
    let mut s = AppState::new(None);
    let bad = [
        (
            Shape::Rect {
                min: p(0.0, 0.0),
                w: 0.0,
                h: 5.0,
                r: 0.0,
            },
            "Rectangle needs a width and a height",
        ),
        (
            Shape::Rect {
                min: p(0.0, 0.0),
                w: -5.0,
                h: 5.0,
                r: 0.0,
            },
            "Rectangle needs a width and a height",
        ),
        (
            Shape::Rect {
                min: p(0.0, 0.0),
                w: 5.0,
                h: f64::NAN,
                r: 0.0,
            },
            "Rectangle needs a width and a height",
        ),
        (
            Shape::Rect {
                min: p(0.0, 0.0),
                w: 5.0,
                h: 1e-12,
                r: 0.0,
            },
            "Rectangle needs a width and a height",
        ),
        (
            Shape::Circle {
                c: p(0.0, 0.0),
                r: -1.0,
            },
            "Circle needs a diameter",
        ),
        (
            Shape::Circle {
                c: p(0.0, 0.0),
                r: f64::INFINITY,
            },
            "Circle needs a diameter",
        ),
        (
            Shape::Line {
                a: p(0.0, 0.0),
                b: p(1e-12, 0.0),
            },
            "Line is too short",
        ),
        (
            Shape::Line {
                a: p(f64::NAN, 0.0),
                b: p(1.0, 0.0),
            },
            "Line is too short",
        ),
    ];
    for (shape, want) in bad {
        assert_eq!(s.add_shape(shape), Err(want.to_string()), "{shape:?}");
    }
    assert!(s.project.items.is_empty() && !s.dirty);
    assert!(
        s.project.layers.is_empty(),
        "no Drawing layer for a rejected shape"
    );
}

#[test]
fn rounded_rect_radius_limits_in_mm_and_inch() {
    let mut s = AppState::new(None);
    // exactly half the short side is a slot, accepted
    assert!(s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 5.0
        })
        .is_ok());
    let big = Shape::Rect {
        min: p(0.0, 0.0),
        w: 30.0,
        h: 10.0,
        r: 5.01,
    };
    assert_eq!(
        s.add_shape(big),
        Err("Corner radius must be at most 5.000 mm".into())
    );
    s.set_units(Units::Inch);
    assert_eq!(
        s.add_shape(big),
        Err("Corner radius must be at most 0.1969 in".into())
    );
}

#[test]
fn negative_corner_radius_message() {
    let mut s = AppState::new(None);
    let e = s
        .add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 10.0,
            h: 10.0,
            r: -1.0,
        })
        .unwrap_err();
    assert!(!e.contains("at most"), "{e}");
}

#[test]
fn two_click_tools_and_tool_switch_drop_pending() {
    let mut s = AppState::new(None);
    s.set_tool(CanvasTool::Rect);
    s.draw.r = 1.0;
    s.canvas_click(p(0.0, 0.0), false);
    s.set_tool(CanvasTool::Circle); // switching drops the first point
    assert_eq!(s.pending, None);
    s.canvas_click(p(1.0, 1.0), false);
    s.canvas_click(p(1.0, 3.0), false);
    let c = contour(&s, s.project.items[0].id);
    assert!((c.as_circle().unwrap().radius - 2.0).abs() < T);
    // a rect from two clicks keeps the form's corner radius
    s.set_tool(CanvasTool::Rect);
    s.canvas_click(p(10.0, 10.0), false);
    s.canvas_click(p(20.0, 15.0), false);
    let id = *s.selection.iter().next().unwrap();
    let k = contour(&s, id);
    assert_eq!(k.segments.len(), 8);
    // a failing second click (radius too big for a thin rect) warns and resets
    s.canvas_click(p(0.0, 0.0), false);
    s.canvas_click(p(10.0, 1.0), false);
    assert_eq!(s.pending, None);
    let m = s.messages.last().unwrap();
    assert_eq!(m.severity, Severity::Warning);
    assert_eq!(m.text, "Corner radius must be at most 0.500 mm");
    // escape in Select still clears the selection (old behaviour kept)
    s.set_tool(CanvasTool::Select);
    assert!(!s.selection.is_empty());
    s.escape();
    assert!(s.selection.is_empty());
    // Select ignores canvas_click
    let n = s.project.items.len();
    s.canvas_click(p(0.0, 0.0), true);
    assert_eq!(s.project.items.len(), n);
}

#[test]
fn set_origin_needs_a_snap_and_works_in_inch() {
    let mut s = AppState::new(None);
    s.set_units(Units::Inch);
    rect(&mut s, 25.4, 50.8, 25.4, 25.4, 0.0);
    s.set_tool(CanvasTool::SetOrigin);
    let sn = s.snap_at(p(25.5, 50.9), 1.0).unwrap();
    s.canvas_click(sn.p, true);
    assert_eq!(s.project.stock.origin, WorkOrigin::Point(p(25.4, 50.8)));
    // the origin is stored in mm whatever the units
    let json = s.project.to_json().unwrap();
    assert!(json.contains("\"x\": 25.4"), "{json}");
}

// ---------------------------------------------------------------- transforms

#[test]
fn rotate_360_and_flip_twice_are_identity() {
    let mut s = AppState::new(None);
    let id = rect(&mut s, 3.0, 4.0, 20.0, 7.0, 2.0);
    let before = contour(&s, id);
    let c = s.selection_center();
    s.transform_selection(&Transform::rotate(360f64.to_radians(), c));
    s.transform_selection(&Transform::mirror_x(c.y));
    s.transform_selection(&Transform::mirror_x(c.y));
    let after = contour(&s, id);
    for (a, b) in before.segments.iter().zip(&after.segments) {
        assert!(a.start().approx_eq(b.start(), 1e-9) && a.midpoint().approx_eq(b.midpoint(), 1e-9));
    }
}

#[test]
fn flip_keeps_climb_direction_of_the_cut() {
    // Outside Climb on a rounded rect: after a flip the stored contour is CW, the cut must still run the same way
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let id = rect(&mut s, 0.0, 0.0, 30.0, 20.0, 4.0);
    let op = s.new_operation(NewKind::Contour, 1).unwrap();
    edit_op(&mut s, op, |o| {
        o.kind = hc_project::OpKind::Contour {
            side: hc_project::Side::Outside,
            direction: hc_project::Direction::Climb,
        };
        o.depth = 1.0;
    });
    s.refresh_toolpaths();
    let a0 = cut_area(&s, op);
    s.selection = [id].into_iter().collect();
    let c = s.selection_center();
    s.transform_selection(&Transform::mirror_y(c.x));
    s.refresh_toolpaths();
    let a1 = cut_area(&s, op);
    assert!(a0.abs() > 1.0);
    assert_eq!(a0.signum(), a1.signum(), "{a0} {a1}");
    // and the toolpath follows the geometry: G-code bbox unchanged by a flip about the centre
    s.transform_selection(&Transform::translate(7.0, 0.0));
    s.refresh_toolpaths();
    assert_eq!(s.toolpaths_for.as_ref(), Some(&s.project));
}

#[test]
fn transform_moves_toolpath_and_manual_tabs_with_the_part() {
    let (mut s, outside) = bracket_clean();
    let edge = outer(&s);
    edit_op(&mut s, outside, |o| {
        o.tabs.manual = vec![ManualTabs {
            item: edge,
            positions: vec![0.1, 0.6],
        }]
    });
    s.select_op(outside);
    let before: Vec<Point> = {
        let (_, c, pos) = s.tab_markers().into_iter().find(|m| m.0 == edge).unwrap();
        pos.iter().map(|t| c.point_at(t * c.length())).collect()
    };
    s.selection = [edge].into_iter().collect();
    s.transform_selection(&Transform::translate(5.0, -2.0));
    let op = s
        .project
        .operations
        .iter()
        .find(|o| o.id == outside)
        .unwrap();
    assert_eq!(op.tabs.manual[0].positions, [0.1, 0.6]);
    let (_, c, pos) = s.tab_markers().into_iter().find(|m| m.0 == edge).unwrap();
    for (t, b) in pos.iter().zip(&before) {
        assert!(c
            .point_at(t * c.length())
            .approx_eq(p(b.x + 5.0, b.y - 2.0), 1e-9));
    }
    // the origin does not move with the part
    assert_eq!(s.project.stock.origin, WorkOrigin::DrawingOrigin);
}

#[test]
fn scale_zero_or_non_finite_is_rejected_by_state() {
    for f in [0.0, f64::NAN, f64::INFINITY] {
        let mut s = AppState::new(None);
        rect(&mut s, 0.0, 0.0, 10.0, 10.0, 0.0);
        let before = s.project.clone();
        let c = s.selection_center();
        s.transform_selection(&Transform::scale(f, c));
        assert_eq!(s.project, before, "factor {f}");
    }
}

// ---------------------------------------------------------------- delete

#[test]
fn deleting_every_item_of_an_op_reports_no_items_and_blocks_export() {
    let (mut s, outside) = bracket_clean();
    s.selected_op = Some(outside);
    s.selection = [outer(&s)].into_iter().collect();
    s.delete_selection();
    s.refresh_toolpaths();
    assert!(s
        .issues
        .iter()
        .any(|i| i.rule == RuleId::NoItems && i.op == Some(outside)));
    assert!(s.gcode().is_none());
    assert!(s
        .messages
        .last()
        .unwrap()
        .text
        .starts_with("Cannot export G-code"));
    // the op and its selection survive; nothing panics on a second delete
    assert_eq!(s.selected_op, Some(outside));
    s.delete_selection();
}

#[test]
fn delete_with_a_selected_tab_does_not_touch_items() {
    let (mut s, outside) = bracket_clean();
    s.select_op(outside);
    let (item, _, _) = s.tab_markers()[0].clone();
    let n = s.project.items.len();
    s.selected_tab = Some((item, 0));
    // the canvas key handler: tab first
    match s.selected_tab {
        Some((i, k)) => s.delete_tab(i, k),
        None => s.delete_selection(),
    }
    assert_eq!(s.project.items.len(), n);
}

// ---------------------------------------------------------------- undo / redo

#[test]
fn every_edit_kind_is_undoable() {
    let (mut s, outside) = bracket_clean();
    s.commit_undo(true);
    let edge = outer(&s);
    type Edit = Box<dyn Fn(&mut AppState)>;
    let edits: Vec<(&str, Edit)> = vec![
        (
            "add_shape",
            Box::new(|s| {
                rect(s, 0.0, 0.0, 5.0, 5.0, 0.0);
            }),
        ),
        (
            "transform",
            Box::new(move |s| {
                s.selection = [edge].into_iter().collect();
                s.transform_selection(&Transform::translate(1.0, 0.0));
            }),
        ),
        (
            "delete",
            Box::new(move |s| {
                s.selection = [edge].into_iter().collect();
                s.delete_selection();
            }),
        ),
        ("units", Box::new(|s| s.set_units(Units::Inch))),
        (
            "machine",
            Box::new(|s| {
                let mut m = s.project.machine.clone();
                m.safe_z += 1.0;
                s.set_machine(m);
            }),
        ),
        (
            "origin point",
            Box::new(|s| s.set_origin(WorkOrigin::Point(p(1.0, 2.0)))),
        ),
        (
            "add_tab",
            Box::new(move |s| {
                s.select_op(outside);
                let (item, at) = {
                    let m = &s.tab_markers()[0];
                    (m.0, m.1.point_at(0.33 * m.1.length()))
                };
                s.add_tab(item, at);
            }),
        ),
        (
            "delete_tab",
            Box::new(move |s| {
                s.select_op(outside);
                let (item, _, _) = s.tab_markers()[0].clone();
                s.delete_tab(item, 0);
            }),
        ),
        (
            "new_operation",
            Box::new(move |s| {
                s.selection = [edge].into_iter().collect();
                s.new_operation(NewKind::Engrave, 1).unwrap();
            }),
        ),
        ("delete_op", Box::new(move |s| s.delete_op(outside))),
    ];
    for (name, edit) in &edits {
        let before = s.project.clone();
        edit(&mut s);
        assert_ne!(s.project, before, "{name} changed nothing");
        s.commit_undo(true);
        assert!(s.undo(), "{name}");
        assert_eq!(s.project, before, "{name}");
        assert!(s.redo(), "{name}");
        assert_ne!(s.project, before, "{name} redo");
        assert!(s.undo());
    }
}

#[test]
fn undo_back_past_the_saved_state_and_redo_to_it_is_clean() {
    let dir = scratch("m10v_saved");
    let path = dir.join("a.hcam");
    let mut s = AppState::new(None);
    rect(&mut s, 0.0, 0.0, 5.0, 5.0, 0.0);
    s.commit_undo(true);
    assert!(s.save_as(&path));
    rect(&mut s, 10.0, 0.0, 5.0, 5.0, 0.0);
    s.commit_undo(true);
    assert!(s.dirty);
    s.undo();
    assert!(!s.dirty);
    s.undo();
    assert!(s.dirty);
    s.redo();
    assert!(!s.dirty);
    // a failed open keeps the history
    assert!(!s.open(&dir.join("missing.hcam")));
    assert!(s.can_undo() && s.can_redo());
}

#[test]
fn undo_mid_drag_restores_the_pre_drag_state() {
    let (mut s, outside) = bracket_clean();
    s.commit_undo(true);
    s.select_op(outside);
    let (item, c, pos) = {
        let m = &s.tab_markers()[0];
        (m.0, m.1.clone(), m.2.clone())
    };
    let before = s.project.clone();
    s.selected_tab = Some((item, 0));
    for k in 1..=5 {
        let at = c.point_at((pos[0] + 0.01 * k as f64) * c.length());
        s.move_tab(item, 0, at);
        s.commit_undo(false); // button still down
    }
    assert_ne!(s.project, before);
    assert!(s.undo());
    assert_eq!(s.project, before);
    assert_eq!(s.selected_tab, None, "the drag loses its tab");
    // the drag code keeps calling move_tab with the old index: must not panic
    let at = c.point_at((pos[0] + 0.2) * c.length());
    s.move_tab(item, 0, at);
    s.commit_undo(true);
    assert!(s.can_undo());
}

#[test]
fn undo_drops_a_selected_op_that_no_longer_exists_and_refreshes_gcode() {
    let (mut s, _) = bracket_clean();
    s.commit_undo(true);
    s.refresh_toolpaths();
    let g0 = s.gcode().unwrap();
    s.selection = [outer(&s)].into_iter().collect();
    let op = s.new_operation(NewKind::Engrave, 1).unwrap();
    s.set_origin(WorkOrigin::Point(p(3.0, 4.0)));
    s.commit_undo(true);
    assert_eq!(s.selected_op, Some(op));
    s.undo();
    assert_eq!(s.selected_op, None);
    s.refresh_toolpaths();
    assert_eq!(s.toolpaths_for.as_ref(), Some(&s.project));
    let g1 = s.gcode().unwrap();
    assert_eq!(
        g0.iter().map(|f| &f.text).collect::<Vec<_>>(),
        g1.iter().map(|f| &f.text).collect::<Vec<_>>()
    );
}

#[test]
fn depth_limit_and_redo_is_unbounded_by_it() {
    let mut s = AppState::new(None);
    for k in 0..(UNDO_DEPTH + 3) {
        s.set_origin(WorkOrigin::Point(p(k as f64 + 1.0, 0.0)));
        s.commit_undo(true);
    }
    assert_eq!(s.undo.len(), UNDO_DEPTH);
    while s.undo() {}
    assert_eq!(s.redo.len(), UNDO_DEPTH);
    // oldest reachable: origin k = 2 (states 0..=2 dropped: fresh, 1, 2)
    assert_eq!(s.project.stock.origin, WorkOrigin::Point(p(3.0, 0.0)));
    while s.redo() {}
    assert_eq!(
        s.project.stock.origin,
        WorkOrigin::Point(p(UNDO_DEPTH as f64 + 3.0, 0.0))
    );
}

#[test]
fn view_and_tool_state_are_not_undone() {
    let mut s = AppState::new(None);
    rect(&mut s, 0.0, 0.0, 5.0, 5.0, 0.0);
    s.commit_undo(true);
    s.view.zoom = 3.0;
    s.set_tool(CanvasTool::Circle);
    s.draw.d = 42.0;
    s.commit_undo(true);
    assert_eq!(s.undo.len(), 1, "non-project state committed nothing");
    s.undo();
    assert_eq!(s.view.zoom, 3.0);
    assert_eq!(s.tool, CanvasTool::Circle);
    assert_eq!(s.draw.d, 42.0);
}

// ---------------------------------------------------------------- kittest

mod ui {
    use super::*;
    use crate::App;
    use eframe::egui;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    fn harness(s: AppState) -> Harness<'static, App> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
    }

    /// Screen position of world point `w` (the canvas rect is not stored; probe with a hover).
    fn screen_of(h: &mut Harness<'_, App>, w: Point) -> egui::Pos2 {
        let probe = egui::pos2(640.0, 400.0);
        h.hover_at(probe);
        h.run();
        let s = &h.state().state;
        let c = s.cursor.expect("canvas hovered");
        let z = s.view.zoom;
        probe + egui::vec2(((w.x - c.x) as f32) * z, -((w.y - c.y) as f32) * z)
    }

    fn click(h: &mut Harness<'_, App>, at: egui::Pos2) {
        h.hover_at(at);
        h.run();
        h.drag_at(at);
        h.run();
        h.drop_at(at);
        h.run();
    }

    fn corner_state() -> AppState {
        let mut s = AppState::new(None);
        rect(&mut s, 0.0, 0.0, 40.0, 30.0, 0.0);
        s.clear_selection();
        s.commit_undo(true);
        s
    }

    #[test]
    fn set_origin_click_snaps_within_10_px_at_any_zoom() {
        for zoom in [0.5f32, 4.0] {
            let mut h = harness(corner_state());
            h.run();
            h.state_mut().state.view.zoom = zoom;
            h.state_mut().state.set_tool(CanvasTool::SetOrigin);
            h.run();
            // 14 px left of the bottom-left corner: no snap, nothing happens
            let at = screen_of(&mut h, p(0.0, 0.0)) + egui::vec2(-14.0, 0.0);
            click(&mut h, at);
            let s = &h.state().state;
            assert_eq!(
                s.project.stock.origin,
                WorkOrigin::DrawingOrigin,
                "zoom {zoom}"
            );
            assert_eq!(s.tool, CanvasTool::SetOrigin);
            // 7 px away (outside the part): exactly the corner
            let at = screen_of(&mut h, p(0.0, 0.0)) + egui::vec2(-5.0, 5.0);
            click(&mut h, at);
            let s = &h.state().state;
            assert_eq!(
                s.project.stock.origin,
                WorkOrigin::Point(p(0.0, 0.0)),
                "zoom {zoom}"
            );
            assert_eq!(s.tool, CanvasTool::Select);
            assert!(s.can_undo());
        }
    }

    #[test]
    fn drawing_clicks_use_the_snap_exactly() {
        let mut h = harness(corner_state());
        h.run();
        h.state_mut().state.set_tool(CanvasTool::Line);
        h.run();
        let a = screen_of(&mut h, p(40.0, 30.0)) + egui::vec2(-3.0, 2.0);
        click(&mut h, a);
        assert_eq!(h.state().state.pending, Some(p(40.0, 30.0)), "{a:?}");
        let b = screen_of(&mut h, p(40.0, 30.0)) + egui::vec2(-60.0, -60.0);
        click(&mut h, b);
        let s = &h.state().state;
        assert_eq!(s.project.items.len(), 2);
        let Geom::Contour(c) = &s.project.items[1].geom else {
            panic!()
        };
        assert_eq!(c.start(), p(40.0, 30.0));
        assert!(!c.closed);
    }

    #[test]
    fn backspace_in_a_field_does_not_delete_the_selection() {
        let mut s = corner_state();
        s.selection = [s.project.items[0].id].into_iter().collect();
        let mut h = harness(s);
        h.run();
        h.get_by_label("Move X").focus();
        h.run();
        h.hover_at(egui::pos2(640.0, 400.0));
        h.run();
        assert!(h.ctx.egui_wants_keyboard_input());
        h.key_press(egui::Key::Backspace);
        h.run();
        h.key_press(egui::Key::Delete);
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);
    }

    #[test]
    fn tool_letters_in_a_field_do_not_switch_tools() {
        let mut s = corner_state();
        s.set_tool(CanvasTool::Rect);
        let mut h = harness(s);
        h.run();
        h.get_by_label("Width").focus();
        h.run();
        h.hover_at(egui::pos2(640.0, 400.0));
        h.run();
        for k in [egui::Key::L, egui::Key::C, egui::Key::O] {
            h.key_press(k);
            h.run();
        }
        assert_eq!(h.state().state.tool, CanvasTool::Rect);
    }

    #[test]
    fn scale_disabled_for_zero_and_negative_factor() {
        let mut s = corner_state();
        s.selection = [s.project.items[0].id].into_iter().collect();
        for f in [0.0, -2.0] {
            s.xform.factor = f;
            let mut h = harness(clone_state(&s));
            h.run();
            assert!(
                h.get_by_label("Scale").accesskit_node().is_disabled(),
                "{f}"
            );
        }
        s.xform.factor = 1.5;
        let mut h = harness(clone_state(&s));
        h.run();
        assert!(!h.get_by_label("Scale").accesskit_node().is_disabled());
    }

    /// A fresh state with the same project, selection and transform form.
    fn clone_state(s: &AppState) -> AppState {
        let mut n = AppState::new(None);
        n.project = s.project.clone();
        n.commit_undo(true);
        n.undo();
        n.redo(); // items_changed
        n.selection = s.selection.clone();
        n.xform = s.xform;
        n
    }

    #[test]
    fn undo_menu_enabled_only_with_history() {
        let mut h = harness(AppState::new(None));
        h.run();
        h.get_by_label("Edit").click();
        h.run();
        assert!(crate::kittest::menu(&h, "Undo")
            .accesskit_node()
            .is_disabled());
        h.key_press(egui::Key::Escape);
        h.run();
        h.state_mut().state.set_origin(WorkOrigin::StockCenter);
        h.run();
        h.get_by_label("Edit").click();
        h.run();
        assert!(!crate::kittest::menu(&h, "Undo")
            .accesskit_node()
            .is_disabled());
        crate::kittest::menu(&h, "Undo").click();
        h.run();
        assert_eq!(
            h.state().state.project.stock.origin,
            WorkOrigin::DrawingOrigin
        );
    }
}
