//! M20 acceptance tests: move by key points, object/grid snapping, prefs.

use crate::snap::SnapKind;
use crate::state::{AppState, CanvasTool, Severity, Shape};
use hc_geom::Point;
use hc_project::{Geom, Prefs, Units, WorkOrigin};
use std::collections::BTreeSet;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn near(a: Point, b: Point) -> bool {
    a.approx_eq(b, 1e-9)
}

struct Ids {
    l: u64,
    c: u64,
    r: u64,
    x1: u64,
    x2: u64,
}

fn snap20() -> (AppState, Ids) {
    let mut s = AppState::new(None);
    s.view.zoom = 2.0;
    let line = |s: &mut AppState, a, b| s.add_shape(Shape::Line { a, b }).unwrap()[0];
    let l = line(&mut s, p(0.0, 0.0), p(40.0, 0.0));
    let c = s
        .add_shape(Shape::Circle {
            c: p(100.0, 50.0),
            r: 10.0,
        })
        .unwrap()[0];
    let r = s
        .add_shape(Shape::Rect {
            min: p(200.0, 0.0),
            w: 30.0,
            h: 20.0,
            r: 0.0,
        })
        .unwrap()[0];
    let x1 = line(&mut s, p(400.0, 0.0), p(460.0, 30.0));
    let x2 = line(&mut s, p(400.0, 30.0), p(420.0, 0.0));
    s.clear_selection();
    s.dirty = false;
    s.commit_undo(true);
    (s, Ids { l, c, r, x1, x2 })
}

fn centre(s: &AppState, id: u64) -> Point {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.as_circle().unwrap().center,
        Geom::Point(q) => *q,
    }
}

fn grab(s: &AppState, x: f64, y: f64) -> Option<(Point, SnapKind, u64)> {
    s.grab_at(p(x, y), 0.5).map(|g| (g.p, g.kind, g.item))
}

#[test]
fn s1_grab_point() {
    let (mut s, i) = snap20();
    for prefs_off in [false, true] {
        if prefs_off {
            s.prefs.snap_objects = false;
            s.prefs.snap_grid = false;
        }
        assert_eq!(
            grab(&s, 110.0, 50.0),
            Some((p(100.0, 50.0), SnapKind::Center, i.c))
        );
        assert_eq!(
            grab(&s, 15.0, 0.1),
            Some((p(20.0, 0.0), SnapKind::Midpoint, i.l))
        );
        assert_eq!(grab(&s, 4.0, 0.0), Some((p(0.0, 0.0), SnapKind::End, i.l)));
        assert_eq!(grab(&s, 10.0, 0.0), Some((p(0.0, 0.0), SnapKind::End, i.l)));
        assert_eq!(
            grab(&s, 230.2, 13.0),
            Some((p(230.0, 10.0), SnapKind::Midpoint, i.r))
        );
        assert_eq!(
            grab(&s, 230.2, 17.0),
            Some((p(230.0, 20.0), SnapKind::Corner, i.r))
        );
        assert_eq!(grab(&s, 500.0, 500.0), None);
        s.selection = BTreeSet::from([i.x1, i.x2]);
        let g = s.grab_at(p(416.0, 8.0), 0.5).unwrap();
        assert!(near(g.p, p(415.0, 7.5)) && g.kind == SnapKind::Intersection);
        s.selection = BTreeSet::from([i.x1]);
        assert_eq!(
            grab(&s, 416.0, 8.0),
            Some((p(430.0, 15.0), SnapKind::Midpoint, i.x1))
        );
        s.selection = BTreeSet::from([i.l, i.c]);
        let g = s.grab_at(p(100.0, 60.2), 0.5).unwrap();
        assert!(g.p == p(100.0, 50.0) && g.kind == SnapKind::Center);
        let g = s.grab_at(p(39.0, 0.2), 0.5).unwrap();
        assert!(g.p == p(40.0, 0.0) && g.kind == SnapKind::End);
        s.selection.clear();
    }
}

#[test]
fn s2_start_move_grabs_hover_point() {
    let (mut s, i) = snap20();
    assert!(s.start_move(p(110.0, 50.0), 0.5));
    assert_eq!(s.selection, BTreeSet::from([i.c]));
    assert_eq!(s.moving.as_ref().unwrap().grab, p(100.0, 50.0));
}

fn moved(setup: impl Fn(&mut AppState), cursor: Point) -> (AppState, Ids) {
    let (mut s, i) = snap20();
    setup(&mut s);
    assert!(s.start_move(p(110.0, 50.0), 0.5));
    s.drag_move(cursor, 5.0, false);
    (s, i)
}

#[test]
fn s3_drop_order() {
    // (a) object beats grid
    let (s, i) = moved(|_| {}, p(201.0, 1.0));
    let sn = s.moving.as_ref().unwrap().snap.unwrap();
    assert_eq!((sn.kind, sn.item), (SnapKind::Corner, i.r));
    assert!(near(centre(&s, i.c), p(200.0, 0.0)));
    assert!(near(s.moving.as_ref().unwrap().delta, p(100.0, -50.0)));
    // (b) grid
    let (s, i) = moved(|_| {}, p(153.0, 76.0));
    let sn = s.moving.as_ref().unwrap().snap.unwrap();
    assert_eq!((sn.kind, sn.item), (SnapKind::Grid, 0));
    assert!(near(sn.p, p(150.0, 80.0)) && near(centre(&s, i.c), p(150.0, 80.0)));
    // (c)
    let (s, _) = moved(|s| s.prefs.snap_objects = false, p(213.5, 1.0));
    let sn = s.moving.as_ref().unwrap().snap.unwrap();
    assert!(sn.kind == SnapKind::Grid && near(sn.p, p(210.0, 0.0)));
    let (s, _) = moved(|_| {}, p(213.5, 1.0));
    let sn = s.moving.as_ref().unwrap().snap.unwrap();
    assert!(sn.kind == SnapKind::Midpoint && near(sn.p, p(215.0, 0.0)));
    // (d)
    let (s, i) = moved(
        |s| {
            s.prefs.snap_objects = false;
            s.prefs.snap_grid = false;
        },
        p(213.5, 1.0),
    );
    assert!(s.moving.as_ref().unwrap().snap.is_none());
    assert!(near(centre(&s, i.c), p(213.5, 1.0)));
    // (e)
    let (s, i) = moved(|s| s.prefs.snap_grid = false, p(100.5, 50.5));
    assert!(s.moving.as_ref().unwrap().snap.is_none());
    assert!(near(centre(&s, i.c), p(100.5, 50.5)));
    let (s, _) = moved(|_| {}, p(100.5, 50.5));
    let sn = s.moving.as_ref().unwrap().snap.unwrap();
    assert!(sn.kind == SnapKind::Grid && near(sn.p, p(100.0, 50.0)));
}

#[test]
fn s4_shift_after_grid() {
    let (mut s, _) = snap20();
    let d = s
        .add_shape(Shape::Circle {
            c: p(503.3, 47.1),
            r: 5.0,
        })
        .unwrap()[0];
    s.clear_selection();
    assert!(s.start_move(p(508.3, 47.1), 0.5));
    assert!(near(s.moving.as_ref().unwrap().grab, p(503.3, 47.1)));
    s.drag_move(p(531.0, 49.0), 5.0, true);
    let m = s.moving.as_ref().unwrap();
    let sn = m.snap.unwrap();
    assert!(sn.kind == SnapKind::Grid && near(sn.p, p(530.0, 50.0)));
    assert!(near(m.delta, p(26.7, 0.0)));
    assert!(near(centre(&s, d), p(530.0, 47.1)));
}

#[test]
fn s5_shared_rule_for_tools() {
    let (mut s, _) = snap20();
    s.tool = CanvasTool::Line;
    s.snapped_click(p(0.6, 0.4));
    assert_eq!(s.pending, Some(p(0.0, 0.0)));
    let n = s.project.items.len();
    s.snapped_click(p(57.0, 23.0));
    assert_eq!(s.project.items.len(), n + 1);
    match &s.project.items.last().unwrap().geom {
        Geom::Contour(c) => {
            assert!(c.start().approx_eq(p(0.0, 0.0), 1e-9));
            assert!(c.end().approx_eq(p(60.0, 20.0), 1e-9));
        }
        Geom::Point(_) => panic!(),
    }
    let (mut s, _) = snap20();
    s.prefs.snap_grid = false;
    s.tool = CanvasTool::Line;
    s.snapped_click(p(57.0, 23.0));
    assert_eq!(s.pending, Some(p(57.0, 23.0)));
    // Measure
    let (mut s, _) = snap20();
    s.tool = CanvasTool::Measure;
    s.pending = Some(p(0.0, 0.0));
    s.cursor = Some(p(57.0, 23.0));
    assert_eq!(s.measure_live(), Some((p(0.0, 0.0), p(60.0, 20.0))));
    s.prefs.snap_grid = false;
    assert_eq!(s.measure_live(), Some((p(0.0, 0.0), p(57.0, 23.0))));
    // snap_point
    let (mut s, _) = snap20();
    let sp = s.snap_point(p(0.6, 0.4), 5.0).unwrap();
    assert!(sp.kind == SnapKind::End && sp.p == p(0.0, 0.0));
    s.prefs.snap_objects = false;
    let sp = s.snap_point(p(0.6, 0.4), 5.0).unwrap();
    assert!(sp.kind == SnapKind::Grid && sp.p == p(0.0, 0.0));
    s.prefs.snap_grid = false;
    assert!(s.snap_point(p(0.6, 0.4), 5.0).is_none());
}

#[test]
fn s6_set_origin() {
    let (mut s, _) = snap20();
    s.tool = CanvasTool::SetOrigin;
    s.snapped_click(p(57.0, 23.0));
    assert_eq!(s.project.stock.origin, WorkOrigin::Point(p(60.0, 20.0)));
    assert_eq!(s.tool, CanvasTool::Select);

    let (mut s, _) = snap20();
    let before = s.project.stock.origin;
    s.prefs.snap_grid = false;
    s.tool = CanvasTool::SetOrigin;
    s.snapped_click(p(57.0, 23.0));
    assert_eq!(s.project.stock.origin, before);
    assert_eq!(s.tool, CanvasTool::SetOrigin);

    let (mut s, _) = snap20();
    s.prefs.snap_grid = false;
    s.prefs.snap_objects = false;
    s.tool = CanvasTool::SetOrigin;
    s.snapped_click(p(57.0, 23.0));
    assert_eq!(s.project.stock.origin, WorkOrigin::Point(p(57.0, 23.0)));
}

#[test]
fn s7_prefs_in_state() {
    let dir = crate::state::tests::scratch("m20_prefs");
    let lib = dir.join("tools.json");
    let mut s = AppState::new(Some(lib.clone()));
    assert_eq!(s.prefs, Prefs::default());
    assert!(!s.messages.iter().any(|m| m.text.starts_with("Preferences")));
    assert_eq!(s.messages.len(), 0);
    let (n, base) = (s.undo.len(), s.undo_base.clone());
    s.set_prefs(Prefs {
        snap_grid: false,
        ..s.prefs
    });
    assert!(dir.join("prefs.json").exists());
    assert!(!s.dirty && s.undo.len() == n && s.project == base);
    assert!(!AppState::new(Some(lib)).prefs.snap_grid);
    // (c) unchanged value writes nothing
    let d2 = crate::state::tests::scratch("m20_prefs_c");
    let mut s = AppState::new(Some(d2.join("tools.json")));
    s.set_prefs(s.prefs);
    assert!(!d2.join("prefs.json").exists());
    // (d) damaged file
    let d3 = crate::state::tests::scratch("m20_prefs_d");
    std::fs::write(d3.join("prefs.json"), "{not json").unwrap();
    let s = AppState::new(Some(d3.join("tools.json")));
    assert_eq!(s.prefs, Prefs::default());
    assert_eq!(s.messages.len(), 1);
    assert_eq!(s.messages[0].severity, Severity::Warning);
    assert!(s.messages[0].text.starts_with("Preferences: cannot load "));
    // (e) failed save
    let d4 = crate::state::tests::scratch("m20_prefs_e");
    std::fs::write(d4.join("blocker"), "x").unwrap();
    let mut s = AppState::new(Some(d4.join("tools.json")));
    s.messages.clear();
    s.prefs_path = Some(d4.join("blocker").join("prefs.json"));
    s.set_prefs(Prefs {
        snap_objects: false,
        ..s.prefs
    });
    assert!(!s.prefs.snap_objects && !s.dirty);
    assert_eq!(s.messages.len(), 1);
    assert_eq!(s.messages[0].severity, Severity::Warning);
    assert!(s.messages[0].text.starts_with("Cannot save preferences: "));
    // (f) in memory only
    let mut s = AppState::new(None);
    let n = s.messages.len();
    assert!(s.prefs_path.is_none());
    s.set_prefs(Prefs {
        snap_grid: false,
        ..s.prefs
    });
    assert!(!s.prefs.snap_grid);
    assert_eq!(s.messages.len(), n);
}

#[test]
fn s8_grid_step() {
    let (mut s, _) = snap20();
    assert_eq!(s.grid_step(), 10.0);
    s.project.units = Units::Inch;
    s.view.zoom = 10.0;
    assert!((s.grid_step() - 2.54).abs() < 1e-9);
}

mod kittest {
    use super::*;
    use crate::view::View;
    use crate::App;
    use eframe::egui::{self, PointerButton};
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;

    fn fixed() -> Harness<'static, App> {
        let mut s = AppState::new(None);
        s.add_shape(Shape::Line {
            a: p(0.0, 0.0),
            b: p(40.0, 0.0),
        })
        .unwrap();
        s.add_shape(Shape::Circle {
            c: p(100.0, 50.0),
            r: 10.0,
        })
        .unwrap();
        s.add_shape(Shape::Rect {
            min: p(150.0, 0.0),
            w: 30.0,
            h: 20.0,
            r: 0.0,
        })
        .unwrap();
        s.clear_selection();
        s.dirty = false;
        s.commit_undo(true);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        let st = &mut h.state_mut().state;
        st.fit_pending = false;
        st.view = View {
            zoom: 2.0,
            offset: egui::vec2(100.0, 500.0),
        };
        h.run();
        h
    }

    fn to_s(h: &Harness<App>, q: Point) -> egui::Pos2 {
        h.state()
            .state
            .view
            .world_to_screen(h.state().canvas_rect, q)
    }

    fn btn(h: &Harness<App>, pos: egui::Pos2, pressed: bool) {
        h.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }

    fn drag(h: &mut Harness<'static, App>, a: Point, b: Point) {
        let (a, b) = (to_s(h, a), to_s(h, b));
        h.hover_at(a);
        h.run();
        btn(h, a, true);
        h.run();
        for k in 1..=4 {
            h.hover_at(a + (b - a) * (k as f32 / 4.0));
            h.run();
        }
        btn(h, b, false);
        h.run();
        h.run();
    }

    fn click(h: &mut Harness<'static, App>, at: Point) {
        let at = to_s(h, at);
        h.hover_at(at);
        h.run();
        btn(h, at, true);
        h.run();
        btn(h, at, false);
        h.run();
        h.run();
    }

    fn circle_id(h: &Harness<App>) -> u64 {
        h.state()
            .state
            .project
            .items
            .iter()
            .find(|i| matches!(&i.geom, Geom::Contour(c) if c.as_circle().is_some()))
            .unwrap()
            .id
    }

    fn checked(h: &Harness<App>, name: &str) -> bool {
        crate::kittest::menu(h, name).accesskit_node().toggled()
            == Some(egui::accesskit::Toggled::True)
    }

    #[test]
    fn k1_hover_marker() {
        let mut h = fixed();
        h.hover_at(to_s(&h, p(110.0, 50.0)));
        h.run();
        let g = h.state().grab_hover.unwrap();
        assert!(g.kind == SnapKind::Center && g.p.approx_eq(p(100.0, 50.0), 1e-9));
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        h.run();
        assert!(h.state().grab_hover.is_none());
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.hover_at(to_s(&h, p(60.0, 100.0)));
        h.run();
        assert!(h.state().grab_hover.is_none());
    }

    #[test]
    fn k2_drag_line_midpoint_onto_corner() {
        let mut h = fixed();
        let n = h.state().state.undo.len();
        drag(&mut h, p(15.0, 0.0), p(150.8, 0.6));
        let s = &h.state().state;
        let l = &s.project.items[0];
        match &l.geom {
            Geom::Contour(c) => {
                assert!(c.start().approx_eq(p(130.0, 0.0), 1e-6), "{:?}", c.start());
                assert!(c.end().approx_eq(p(170.0, 0.0), 1e-6));
            }
            Geom::Point(_) => panic!(),
        }
        assert_eq!(s.undo.len(), n + 1);
        assert!(s.moving.is_none());
    }

    #[test]
    fn k3_drag_onto_grid() {
        let mut h = fixed();
        drag(&mut h, p(110.0, 50.0), p(63.0, 77.0));
        let id = circle_id(&h);
        assert!(centre(&h.state().state, id).approx_eq(p(60.0, 80.0), 1e-6));
    }

    #[test]
    fn k4_settings_toggles() {
        let mut h = fixed();
        h.state_mut().settings_open = true;
        h.run();
        for n in ["Snapping", "Snap to objects", "Snap to grid"] {
            assert!(crate::kittest::menu_q(&h, n, false).is_some(), "{n}");
        }
        assert!(checked(&h, "Snap to objects") && checked(&h, "Snap to grid"));
        let n = h.state().state.undo.len();
        crate::kittest::menu(&h, "Snap to grid").click();
        h.run();
        let s = &h.state().state;
        assert!(!s.prefs.snap_grid && s.prefs.snap_objects);
        assert!(!s.dirty && s.undo.len() == n);
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(!h.state().settings_open);
        let mut h2 = h;
        drag(&mut h2, p(110.0, 50.0), p(63.0, 77.0));
        let id = circle_id(&h2);
        let want = h2
            .state()
            .state
            .view
            .screen_to_world(h2.state().canvas_rect, to_s(&h2, p(63.0, 77.0)));
        assert!(centre(&h2.state().state, id).approx_eq(want, 1e-3));
        h2.state_mut().settings_open = true;
        h2.run();
        crate::kittest::menu(&h2, "Snap to objects").click();
        h2.run();
        assert!(!h2.state().state.prefs.snap_objects);
        h2.key_press(egui::Key::Escape);
        h2.run();
        h2.state_mut().state.set_tool(CanvasTool::Line);
        h2.run();
        click(&mut h2, p(0.6, 0.4));
        let pend = h2.state().state.pending.unwrap();
        assert!(pend.approx_eq(p(0.6, 0.4), 1e-3), "{pend:?}");
    }

    fn hold(h: &mut Harness<'static, App>, a: Point) {
        let a = to_s(h, a);
        h.hover_at(a);
        h.run();
        btn(h, a, true);
        h.run();
    }

    #[test]
    fn k5_hover_marker_hidden_while_dragging_space_and_box() {
        let mut h = fixed();
        let c = to_s(&h, p(110.0, 50.0));
        h.hover_at(c);
        h.run();
        assert!(h.state().grab_hover.is_some());
        // button down: hidden
        btn(&h, c, true);
        h.run();
        h.hover_at(to_s(&h, p(112.0, 50.0)));
        h.run();
        assert!(h.state().grab_hover.is_none(), "while dragging");
        btn(&h, c, false);
        h.run();
        h.run();
        // Space held: hidden
        h.hover_at(to_s(&h, p(110.0, 50.0)));
        h.run();
        h.key_down(egui::Key::Space);
        h.run();
        assert!(h.state().grab_hover.is_none(), "space");
        h.key_up(egui::Key::Space);
        h.run();
    }

    #[test]
    fn k6_hover_marker_only_with_select_tool() {
        let mut h = fixed();
        h.state_mut().state.set_tool(CanvasTool::Line);
        h.hover_at(to_s(&h, p(110.0, 50.0)));
        h.run();
        h.run();
        assert!(h.state().grab_hover.is_none());
    }

    #[test]
    fn k7_one_undo_step_per_drag_and_undo_restores() {
        let mut h = fixed();
        let before = h.state().state.project.clone();
        let n = h.state().state.undo.len();
        drag(&mut h, p(110.0, 50.0), p(63.0, 77.0));
        assert_eq!(h.state().state.undo.len(), n + 1);
        assert!(h.state().state.dirty);
        assert!(h.state_mut().state.undo());
        assert_eq!(h.state().state.project, before);
    }

    #[test]
    fn k8_esc_mid_drag_restores() {
        let mut h = fixed();
        h.state_mut().state.saved = h.state().state.project.clone();
        let before = h.state().state.project.clone();
        let n = h.state().state.undo.len();
        hold(&mut h, p(110.0, 50.0));
        for k in 1..=3 {
            h.hover_at(to_s(&h, p(110.0 - 15.0 * k as f64, 50.0 + 9.0 * k as f64)));
            h.run();
        }
        assert!(h.state().state.moving.is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        btn(&h, to_s(&h, p(65.0, 77.0)), false);
        h.run();
        h.run();
        let s = &h.state().state;
        assert!(s.moving.is_none() && !s.dirty);
        assert_eq!(s.project, before);
        assert_eq!(s.undo.len(), n);
    }

    #[test]
    fn k9_settings_window_fits_with_snapping() {
        let mut h = fixed();
        h.state_mut().settings_open = true;
        h.run();
        assert!(crate::kittest::menu(&h, "Snap to grid").rect().max.y < 800.0);
        assert!(crate::kittest::menu(&h, "Snapping").rect().min.y > 0.0);
    }
}

mod gaps {
    use super::*;
    use crate::state::tests::{bracket_clean, outer};

    fn line_pts(s: &AppState, id: u64) -> (Point, Point) {
        match &s.item(id).unwrap().geom {
            Geom::Contour(c) => (c.start(), c.end()),
            Geom::Point(q) => (*q, *q),
        }
    }

    #[test]
    fn multi_selection_moves_all_by_nearest_key_point() {
        let (mut s, i) = snap20();
        s.prefs.snap_grid = false;
        s.selection = BTreeSet::from([i.l, i.c]);
        // pressing L at (30,0): End (40,0) and Midpoint (20,0) tie -> End
        let g = s.grab_at(p(30.0, 0.0), 0.5).unwrap();
        assert!(g.p == p(40.0, 0.0) && g.kind == SnapKind::End && g.item == i.l);
        assert!(s.start_move(p(30.0, 0.0), 0.5));
        s.drag_move(p(50.0, 10.0), 5.0, false);
        assert!(near(line_pts(&s, i.l).0, p(10.0, 10.0)));
        assert!(near(centre(&s, i.c), p(110.0, 60.0)));
        assert!(near(line_pts(&s, i.r).0, p(200.0, 0.0)), "R untouched");
    }

    #[test]
    fn pressing_unselected_item_moves_only_it() {
        let (mut s, i) = snap20();
        s.prefs.snap_grid = false;
        s.selection = BTreeSet::from([i.c, i.r]);
        assert!(s.start_move(p(15.0, 0.1), 0.5));
        assert_eq!(s.selection, BTreeSet::from([i.l]));
        assert_eq!(s.moving.as_ref().unwrap().orig.len(), 1);
        s.drag_move(p(35.0, 5.0), 5.0, false);
        assert!(near(line_pts(&s, i.l).0, p(15.0, 5.0)));
        assert!(near(centre(&s, i.c), p(100.0, 50.0)));
    }

    #[test]
    fn dxf_point_item_is_grabbed_and_moved() {
        let (mut s, _) = snap20();
        s.prefs.snap_grid = false;
        let id = s.project.add_drawn(vec![Geom::Point(p(300.0, 300.0))])[0];
        s.items_changed();
        let g = s.grab_at(p(300.2, 300.0), 0.5).unwrap();
        assert!(g.p == p(300.0, 300.0) && g.kind == SnapKind::Point && g.item == id);
        assert!(s.start_move(p(300.2, 300.0), 0.5));
        s.drag_move(p(310.0, 305.0), 5.0, false);
        assert_eq!(s.item(id).unwrap().geom, Geom::Point(p(310.0, 305.0)));
    }

    #[test]
    fn text_items_have_grab_points() {
        let (mut s, _) = snap20();
        let ids = s
            .add_shape(Shape::Text {
                at: p(0.0, 200.0),
                text: "HO".into(),
                font: hc_text::Font::Outline,
                height: 10.0,
            })
            .unwrap();
        s.clear_selection();
        for &id in &ids {
            let Geom::Contour(c) = &s.item(id).unwrap().geom else {
                panic!()
            };
            let q = c.start();
            let g = s.grab_at(q, 0.3).expect("stroke start is grabbable");
            assert!(ids.contains(&g.item), "grab belongs to the text");
            assert!(g.p.distance(q) < 10.0);
        }
        // press moves all pressed stroke only; drag works
        let Geom::Contour(c) = &s.item(ids[0]).unwrap().geom else {
            panic!()
        };
        let q = c.start();
        assert!(s.start_move(q, 0.3));
        let before = s.moving.as_ref().unwrap().grab;
        s.drag_move(before + p(7.0, 3.0), 5.0, false);
        s.cancel_move();
    }

    #[test]
    fn tie_point_vs_end_prefers_end_in_either_order() {
        for point_first in [false, true] {
            let (mut s, i) = snap20();
            let id = if point_first {
                // cache order: point item added later anyway; test kind order with a reversed pair
                s.project.add_drawn(vec![Geom::Point(p(0.0, 0.0))])[0]
            } else {
                s.project.add_drawn(vec![Geom::Point(p(40.0, 0.0))])[0]
            };
            s.items_changed();
            s.selection = BTreeSet::from([i.l, id]);
            let q = if point_first {
                p(-1.0, 0.0)
            } else {
                p(41.0, 0.0)
            };
            let g = s.grab_at(q, 3.0).unwrap();
            assert_eq!(g.kind, SnapKind::End, "{point_first}");
            assert_eq!(g.item, i.l);
        }
    }

    #[test]
    fn drop_excludes_all_moving_items_points() {
        let (mut s, i) = snap20();
        s.selection = BTreeSet::from([i.l, i.c]);
        assert!(s.start_move(p(110.0, 50.0), 0.5)); // grab C centre
        s.drag_move(p(41.0, 1.0), 5.0, false); // near L's own end
        let sn = s.moving.as_ref().unwrap().snap.unwrap();
        assert_eq!((sn.kind, sn.item), (SnapKind::Grid, 0));
        assert!(near(sn.p, p(40.0, 0.0)));
        // but a staying item is still a target
        s.drag_move(p(201.0, 1.0), 5.0, false);
        assert_eq!(s.moving.as_ref().unwrap().snap.unwrap().item, i.r);
    }

    #[test]
    fn grid_drop_matches_drawn_lines_at_zooms_and_inch() {
        for units in [Units::Mm, Units::Inch] {
            for zoom in [0.37_f32, 1.0, 2.6, 13.0, 77.0] {
                let (mut s, _) = snap20();
                s.prefs.snap_objects = false;
                s.project.units = units;
                s.view.zoom = zoom;
                let step = s.grid_step();
                assert!(step > 0.0);
                assert!(s.start_move(p(110.0, 50.0), 0.5));
                let cur = p(1234.567, -89.1);
                s.drag_move(cur, 5.0, false);
                let sn = s.moving.as_ref().unwrap().snap.unwrap();
                assert_eq!(sn.kind, SnapKind::Grid);
                let ix = (cur.x / step).round() as i64;
                let iy = (cur.y / step).round() as i64;
                assert_eq!(sn.p.x, ix as f64 * step, "{units:?} {zoom}");
                assert_eq!(sn.p.y, iy as f64 * step, "{units:?} {zoom}");
                assert!(near(centre(&s, snap_c(&s)), sn.p));
            }
        }
    }

    fn snap_c(s: &AppState) -> u64 {
        *s.selection.iter().next().unwrap()
    }

    #[test]
    fn shift_with_grid_locks_to_a_grid_line_and_keeps_other_coordinate() {
        let (mut s, i) = snap20();
        assert!(s.start_move(p(110.0, 50.0), 0.5));
        // delta before lock (+53,+27): Y smaller -> locked; X target 153 -> grid 150
        s.drag_move(p(153.0, 77.0), 5.0, true);
        let m = s.moving.as_ref().unwrap();
        assert!(near(m.delta, p(50.0, 0.0)));
        assert!(near(centre(&s, i.c), p(150.0, 50.0)));
        // Shift off in the same drag unlocks
        s.drag_move(p(153.0, 77.0), 5.0, false);
        assert!(near(centre(&s, i.c), p(150.0, 80.0)));
    }

    #[test]
    fn esc_mid_drag_restores_exactly_with_snaps() {
        let (mut s, i) = snap20();
        s.saved = s.project.clone();
        let before = s.project.clone();
        let n = s.undo.len();
        s.selection = BTreeSet::from([i.l, i.c, i.r]);
        assert!(s.start_move(p(110.0, 50.0), 0.5));
        s.drag_move(p(153.3, 76.1), 5.0, false);
        s.drag_move(p(201.0, 1.0), 5.0, false);
        assert!(s.dirty);
        s.cancel_move();
        assert!(s.moving.is_none() && !s.dirty);
        assert_eq!(s.project, before);
        assert_eq!(s.undo.len(), n);
        assert_eq!(
            s.snap_cache.cands.len(),
            crate::snap::build_cache(&s.project.items).cands.len()
        );
    }

    #[test]
    fn tools_follow_rule_for_each_setting_combination() {
        // (objects, grid, expected for a click at (0.6, 0.4) next to L's end)
        let cases = [
            (true, true, p(0.0, 0.0)),
            (true, false, p(0.0, 0.0)),
            (false, true, p(0.0, 0.0)),
            (false, false, p(0.6, 0.4)),
        ];
        for tool in [CanvasTool::Line, CanvasTool::Rect, CanvasTool::Circle] {
            for (o, g, want) in cases {
                let (mut s, _) = snap20();
                s.prefs.snap_objects = o;
                s.prefs.snap_grid = g;
                s.tool = tool;
                s.snapped_click(p(0.6, 0.4));
                assert_eq!(s.pending, Some(want), "{tool:?} {o} {g}");
            }
        }
        // far from any object: only grid or raw
        for (o, g, want) in [
            (true, true, p(60.0, 20.0)),
            (true, false, p(57.0, 23.0)),
            (false, true, p(60.0, 20.0)),
            (false, false, p(57.0, 23.0)),
        ] {
            let (mut s, _) = snap20();
            s.prefs.snap_objects = o;
            s.prefs.snap_grid = g;
            s.tool = CanvasTool::Measure;
            s.pending = Some(p(0.0, 0.0));
            s.cursor = Some(p(57.0, 23.0));
            assert_eq!(s.measure_live(), Some((p(0.0, 0.0), want)), "{o} {g}");
        }
    }

    #[test]
    fn snap_is_zoom_dependent_radius_and_grid_step() {
        let (mut s, _) = snap20();
        s.view.zoom = 100.0; // step 0.2, object radius 0.1
        s.tool = CanvasTool::Line;
        s.snapped_click(p(1.23, -0.35));
        let q = s.pending.unwrap();
        assert!(near(q, p(1.2, -0.4)) || (q.x - 1.2).abs() < 1e-9 && (q.y + 0.4).abs() < 1e-9);
    }

    #[test]
    fn prefs_newer_version_and_missing_version_warn_in_state() {
        for (name, text) in [
            (
                "m20_prefs_newer",
                r#"{"format_version":2,"snap_grid":false}"#,
            ),
            ("m20_prefs_nover", r#"{"snap_grid":false}"#),
        ] {
            let d = crate::state::tests::scratch(name);
            std::fs::write(d.join("prefs.json"), text).unwrap();
            let s = AppState::new(Some(d.join("tools.json")));
            assert_eq!(s.prefs, Prefs::default(), "{name}");
            assert_eq!(s.messages.len(), 1, "{name}");
            assert_eq!(s.messages[0].severity, Severity::Warning);
            assert!(s.messages[0].text.starts_with("Preferences: cannot load "));
        }
    }

    #[test]
    fn toggling_prefs_keeps_project_and_undo_clean() {
        let (mut s, _) = snap20();
        let (n, base, proj) = (s.undo.len(), s.undo_base.clone(), s.project.clone());
        let nm = s.messages.len();
        for (o, g) in [(false, true), (false, false), (true, false), (true, true)] {
            s.set_prefs(Prefs {
                snap_objects: o,
                snap_grid: g,
                ..s.prefs
            });
            assert!(!s.dirty && s.undo.len() == n && s.undo_base == base && s.project == proj);
        }
        assert_eq!(s.messages.len(), nm);
    }

    #[test]
    fn tab_marker_wins_over_grab_for_unselected_tab_contour() {
        let (mut s, op) = bracket_clean();
        let edge = outer(&s);
        s.select_op(op);
        s.clear_selection();
        let m = s.tab_markers();
        let (_, c, pos) = m.iter().find(|(id, _, _)| *id == edge).unwrap();
        let at = c.point_at(pos[0].rem_euclid(1.0) * c.length());
        assert!(s.grab_at(at, 0.5).is_none(), "tab marker hit");
        s.selection = BTreeSet::from([edge]);
        assert!(s.grab_at(at, 0.5).is_some(), "selected contour is movable");
    }
}
