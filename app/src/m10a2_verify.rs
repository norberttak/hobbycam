//! M10a.2 verifier tests: snap midpoints/intersections and the sidebar.

use crate::snap::{self, SnapKind};
use crate::state::{AppState, CanvasTool, Shape};
use hc_geom::{shapes, Point, Transform};
use hc_project::{Geom, LayerInfo, Units, WorkOrigin};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn line(s: &mut AppState, a: (f64, f64), b: (f64, f64)) -> u64 {
    s.add_shape(Shape::Line {
        a: p(a.0, a.1),
        b: p(b.0, b.1),
    })
    .unwrap()[0]
}

fn inters(s: &AppState) -> Vec<Point> {
    s.snap_cache
        .cands
        .iter()
        .filter(|c| c.3 == SnapKind::Intersection)
        .map(|c| c.2)
        .collect()
}

// ---------------------------------------------------------------- snap

#[test]
fn intersection_near_an_end_follows_the_0_01_mm_rule() {
    for (end_gap, kept) in [(0.005, false), (0.0099, false), (0.011, true), (0.5, true)] {
        let mut s = AppState::new(None);
        line(&mut s, (0.0, 0.0), (10.0, 0.0));
        // a vertical line ending `end_gap` below the horizontal one... crossing it
        line(&mut s, (5.0, -10.0), (5.0, end_gap));
        assert_eq!(inters(&s).len(), usize::from(kept), "gap {end_gap}");
    }
}

#[test]
fn intersection_hidden_with_either_item() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 10.0));
    let b = line(&mut s, (0.0, 10.0), (10.0, 0.0));
    s.project.layers.push(LayerInfo {
        name: "B".into(),
        color: 1,
    });
    s.project
        .items
        .iter_mut()
        .find(|i| i.id == b)
        .unwrap()
        .layer = "B".into();
    let probe = p(5.0, 5.0);
    let sn = s.snap_at(probe, 0.5).unwrap();
    assert_eq!((sn.kind, sn.p), (SnapKind::Intersection, p(5.0, 5.0)));
    assert_eq!(sn.item, a.min(b));
    s.set_layer_visible("B", false);
    let sn = s.snap_at(probe, 0.5);
    assert!(
        sn.is_none_or(|x| x.kind != SnapKind::Intersection),
        "{sn:?}"
    );
    s.set_layer_visible("B", true);
    s.set_layer_visible("Drawing", false);
    let sn = s.snap_at(probe, 0.5);
    assert!(
        sn.is_none_or(|x| x.kind != SnapKind::Intersection),
        "{sn:?}"
    );
}

#[test]
fn intersection_cache_follows_edit_undo_redo() {
    let mut s = AppState::new(None);
    line(&mut s, (0.0, 0.0), (10.0, 10.0));
    s.commit_undo(true);
    let b = line(&mut s, (0.0, 10.0), (10.0, 0.0));
    s.commit_undo(true);
    assert_eq!(inters(&s).len(), 1);
    s.selection = [b].into_iter().collect();
    s.transform_selection(&Transform::translate(100.0, 0.0));
    s.commit_undo(true);
    assert!(inters(&s).is_empty());
    s.undo();
    assert_eq!(inters(&s), [p(5.0, 5.0)]);
    s.undo();
    assert!(inters(&s).is_empty());
    s.redo();
    assert_eq!(inters(&s).len(), 1);
    assert_eq!(s.snap_cache, snap::build_cache(&s.project.items));
}

#[test]
fn capped_flag_clears_when_the_drawing_shrinks() {
    let mut s = AppState::new(None);
    let geoms: Vec<Geom> = (0..(snap::MAX_X_SEGMENTS + 1))
        .map(|k| {
            let x = k as f64 * 2.0;
            Geom::Contour(shapes::line(p(x, 0.0), p(x + 1.0, 0.0)).unwrap())
        })
        .collect();
    let ids = s.project.add_drawn(geoms);
    line(&mut s, (-5.0, -5.0), (-4.0, -4.0)); // items_changed
    assert!(s.snap_cache.capped);
    s.selection = ids.iter().take(10).copied().collect();
    s.delete_selection();
    assert!(!s.snap_cache.capped);
    s.set_tool(CanvasTool::Line);
    assert!(s.tool.hint(false).is_some());
}

#[test]
fn ties_with_the_new_kinds() {
    // a drill point at a crossing: Point beats Intersection
    let mut s = AppState::new(None);
    s.project.add_drawn(vec![Geom::Point(p(5.0, 5.0))]);
    line(&mut s, (0.0, 0.0), (10.0, 10.0));
    line(&mut s, (0.0, 10.0), (10.0, 0.0));
    assert_eq!(s.snap_at(p(5.1, 5.0), 1.0).unwrap().kind, SnapKind::Point);
    // a rect corner at another line's midpoint: Corner beats Midpoint
    let mut s = AppState::new(None);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    s.add_shape(Shape::Rect {
        min: p(5.0, 0.0),
        w: 3.0,
        h: 3.0,
        r: 0.0,
    })
    .unwrap();
    assert_eq!(s.snap_at(p(5.1, 0.1), 1.0).unwrap().kind, SnapKind::Corner);
    // a midpoint strictly nearer than a corner wins (nearest first)
    assert_eq!(
        s.snap_at(p(6.5, -0.01), 2.0).unwrap().kind,
        SnapKind::Midpoint
    );
}

#[test]
fn arc_midpoints_ccw_cw_and_slot() {
    // rounded rect r 2: the bottom-left corner arc (centre (2,2), π..3π/2) has its midpoint at 225°
    let k = 2.0 - 2.0 / 2f64.sqrt();
    for mirror in [false, true] {
        let mut s = AppState::new(None);
        let id = s
            .add_shape(Shape::Rect {
                min: p(0.0, 0.0),
                w: 10.0,
                h: 10.0,
                r: 2.0,
            })
            .unwrap()[0];
        if mirror {
            // CW contour after a flip about x = 5: the same physical corners
            s.selection = [id].into_iter().collect();
            s.transform_selection(&Transform::mirror_y(5.0));
        }
        let mids: Vec<Point> = s
            .snap_cache
            .cands
            .iter()
            .filter(|c| c.3 == SnapKind::Midpoint)
            .map(|c| c.2)
            .collect();
        assert_eq!(mids.len(), 8, "mirror {mirror}");
        for want in [
            p(k, k),
            p(10.0 - k, k),
            p(k, 10.0 - k),
            p(10.0 - k, 10.0 - k),
        ] {
            assert!(
                mids.iter().any(|m| m.approx_eq(want, 1e-9)),
                "mirror {mirror}: {want:?} in {mids:?}"
            );
        }
    }
    // slot (r = h/2): each end is two quarter arcs, so its extreme point is a Corner (the zero-length edge is omitted)
    let mut s = AppState::new(None);
    s.add_shape(Shape::Rect {
        min: p(0.0, 0.0),
        w: 20.0,
        h: 6.0,
        r: 3.0,
    })
    .unwrap();
    let sn = s.snap_at(p(20.1, 3.0), 0.5).unwrap();
    assert_eq!(sn.kind, SnapKind::Corner);
    assert!(sn.p.approx_eq(p(20.0, 3.0), 1e-9));
}

#[test]
fn line_tangent_to_and_through_a_circle() {
    let mut s = AppState::new(None);
    s.add_shape(Shape::Circle {
        c: p(0.0, 0.0),
        r: 5.0,
    })
    .unwrap();
    line(&mut s, (-10.0, 5.0), (10.0, 5.0));
    line(&mut s, (-10.0, 0.0), (10.0, 0.0));
    let x = inters(&s);
    for want in [p(0.0, 5.0), p(-5.0, 0.0), p(5.0, 0.0)] {
        assert!(
            x.iter().any(|q| q.approx_eq(want, 1e-9)),
            "{want:?} in {x:?}"
        );
    }
    // the circle's own start point (5,0) is no End/Corner, so the crossing there is kept
    assert_eq!(
        s.snap_at(p(5.05, 0.0), 0.5).unwrap().kind,
        SnapKind::Intersection
    );
}

#[test]
fn inch_project_intersections_are_in_mm() {
    let mut s = AppState::new(None);
    s.set_units(Units::Inch);
    line(&mut s, (0.0, 0.0), (25.4, 25.4));
    line(&mut s, (0.0, 25.4), (25.4, 0.0));
    s.set_tool(CanvasTool::SetOrigin);
    let sn = s.snap_at(p(12.8, 12.6), 1.0).unwrap();
    s.canvas_click(sn.p, true);
    assert_eq!(s.project.stock.origin, WorkOrigin::Point(p(12.7, 12.7)));
}

// ---------------------------------------------------------------- sidebar (kittest)

mod ui {
    use super::*;
    use crate::kittest::{menu_q, side};
    use crate::App;
    use eframe::egui;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    fn toggled(n: &egui_kittest::Node<'_>) -> bool {
        n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    }

    fn harness(s: AppState) -> Harness<'static, App> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
    }

    fn rect_state() -> AppState {
        let mut s = AppState::new(None);
        s.add_shape(Shape::Rect {
            min: p(3.0, 4.0),
            w: 20.0,
            h: 6.0,
            r: 1.0,
        })
        .unwrap();
        s.commit_undo(true);
        s
    }

    /// The project after clicking sidebar `name` vs after `f` on the same starting state.
    fn same_as(name: &str, f: impl Fn(&mut AppState)) {
        let mut h = harness(rect_state());
        h.run();
        side(&h, name).click();
        h.run();
        let mut want = rect_state();
        f(&mut want);
        assert_eq!(h.state().state.project, want.project, "{name}");
    }

    #[test]
    fn edit_buttons_do_exactly_the_transform_group_calls() {
        same_as("Rotate selection", |s| {
            let c = s.selection_center();
            s.transform_selection(&Transform::rotate(s.xform.angle_deg.to_radians(), c))
        });
        same_as("Flip selection horizontally", |s| {
            let c = s.selection_center();
            s.transform_selection(&Transform::mirror_y(c.x))
        });
        same_as("Flip selection vertically", |s| {
            let c = s.selection_center();
            s.transform_selection(&Transform::mirror_x(c.y))
        });
        same_as("Delete selection", |s| s.delete_selection());
        // and the panel buttons of the Transform group agree
        let mut h = harness(rect_state());
        h.run();
        h.get_by_label("Rotate").click();
        h.run();
        let mut want = rect_state();
        want.rotate_selection();
        assert_eq!(h.state().state.project, want.project);
    }

    #[test]
    fn undo_redo_enabled_state_matches_the_menu() {
        let mut h = harness(rect_state());
        h.run();
        for step in 0..3 {
            let side_undo = !side(&h, "Undo").accesskit_node().is_disabled();
            let side_redo = !side(&h, "Redo").accesskit_node().is_disabled();
            h.get_by_label("Edit").click();
            h.run();
            let m_undo = !menu_q(&h, "Undo", false)
                .unwrap()
                .accesskit_node()
                .is_disabled();
            let m_redo = !menu_q(&h, "Redo", false)
                .unwrap()
                .accesskit_node()
                .is_disabled();
            assert_eq!((side_undo, side_redo), (m_undo, m_redo), "step {step}");
            h.key_press(egui::Key::Escape);
            h.run();
            side(&h, "Undo").click();
            h.run();
        }
    }

    #[test]
    fn stock_section_sets_the_origin() {
        let mut h = harness(rect_state());
        h.run();
        side(&h, "Settings…").click();
        h.run();
        h.get_by_label("Stock center").click();
        h.run();
        assert_eq!(
            h.state().state.project.stock.origin,
            WorkOrigin::StockCenter
        );
        assert!(h.state().state.dirty);
    }

    #[test]
    fn units_section_switches_units() {
        let mut h = harness(rect_state());
        h.run();
        side(&h, "Settings…").click();
        h.run();
        let inch: Vec<_> = h.query_all_by_label_contains("inch").collect();
        assert!(!inch.is_empty(), "no inch entry");
        inch[0].click();
        h.run();
        assert_eq!(h.state().state.project.units, Units::Inch);
    }

    #[test]
    fn shortcuts_still_work_with_the_sidebar() {
        let mut h = harness(rect_state());
        h.run();
        h.hover_at(egui::pos2(640.0, 400.0));
        h.run();
        h.key_press(egui::Key::L);
        h.run();
        assert_eq!(h.state().state.tool, CanvasTool::Line);
        assert!(toggled(&side(&h, "Line")));
        h.key_press(egui::Key::Escape);
        h.run();
        assert_eq!(h.state().state.tool, CanvasTool::Select);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert!(h.state().state.project.items.is_empty());
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Y);
        h.run();
        assert_eq!(h.state().state.project.items.len(), 1);
    }

    #[test]
    fn set_origin_button_toggles_back_after_a_pick() {
        let mut h = harness(rect_state());
        h.run();
        side(&h, "Set origin").click();
        h.run();
        assert_eq!(h.state().state.tool, CanvasTool::SetOrigin);
        h.state_mut().state.canvas_click(p(3.0, 4.0), true);
        h.run();
        let s = &h.state().state;
        assert_eq!(s.tool, CanvasTool::Select);
        assert!(toggled(&side(&h, "Select")));
        assert!(!toggled(&side(&h, "Set origin")));
    }
}
