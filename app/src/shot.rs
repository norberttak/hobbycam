//! Offscreen screenshots for visual review (no screen-recording permission needed).
//! Run: HOBBYCAM_SHOT_DIR=/some/dir cargo test -p hobbycam shot -- --ignored

use crate::state::{self, AppState, NewKind};
use crate::App;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn save(h: &mut Harness<'_, App>, name: &str) {
    let dir = std::env::var("HOBBYCAM_SHOT_DIR").expect("set HOBBYCAM_SHOT_DIR");
    let img = h.render().expect("render");
    img.save(format!("{dir}/{name}.png")).expect("save png");
}

#[test]
#[ignore]
fn shot_bracket() {
    let mut s = AppState::new(None);
    s.open_cli(&state::tests::bracket_path());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
    h.run();
    save(&mut h, "bracket");
    h.get_by_label("File").click();
    h.run();
    save(&mut h, "file_menu");
}

use state::tests::circles;

/// M5 test 17. Manual check: feed vs rapid clearly different, arcs smooth, no path through islands.
/// The M5 test 17 project: Outside, Pocket, Drill (T1 Ø6.35) and a failing Inside (T2 Ø20, selected).
fn toolpaths_state() -> AppState {
    let mut s = AppState::new(None);
    let mut t1 = state::tests::tool(1);
    t1.diameter = 6.35;
    t1.depth_per_pass = 3.0;
    s.library.add(t1).unwrap();
    let mut t2 = state::tests::tool(2);
    t2.diameter = 20.0;
    s.library.add(t2).unwrap();
    s.open_cli(&state::tests::bracket_path());
    let outer = s
        .project
        .items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            hc_project::Geom::Contour(c) if c.closed => Some((i.id, c.signed_area().abs())),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0;
    let holes = circles(&s, 15.875);
    let op = |s: &mut AppState, items: Vec<u64>, kind: NewKind, tool: u32| {
        s.selection = items.into_iter().collect();
        s.new_operation(kind, tool).unwrap()
    };
    op(&mut s, vec![outer], NewKind::Contour, 1);
    let pocket = [circles(&s, 76.2), circles(&s, 44.45)].concat();
    op(&mut s, pocket, NewKind::Pocket, 1);
    op(&mut s, holes.clone(), NewKind::Drill, 1);
    let inside = op(&mut s, vec![holes[0]], NewKind::Contour, 2);
    let mut o = s
        .project
        .operations
        .iter()
        .find(|o| o.id == inside)
        .unwrap()
        .clone();
    o.kind = hc_project::OpKind::Contour {
        side: hc_project::Side::Inside,
        direction: hc_project::Direction::Climb,
    };
    s.update_op(o);
    s.clear_selection();
    s.select_op(inside); // render the op properties panel too
    s
}

#[test]
#[ignore]
fn shot_toolpaths() {
    let s = toolpaths_state();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
    h.run();
    assert!(h.query_by_label_contains("Tool too large").is_some());
    assert_eq!(
        h.state()
            .state
            .toolpaths
            .iter()
            .filter(|(_, r)| r.is_ok())
            .count(),
        3
    );
    save(&mut h, "toolpaths");
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Show toolpaths").click();
    h.run();
    assert!(!h.state().state.show_toolpaths);
    // close the menu so the canvas is unobstructed
    h.key_press(egui::Key::Escape);
    h.run();
    save(&mut h, "toolpaths_hidden");
}

/// M6 test 14. Manual check: 4 markers on straight edges, the selected one orange.
#[test]
#[ignore]
fn shot_tabs() {
    let mut s = AppState::new(None);
    let mut t1 = state::tests::tool(1);
    t1.diameter = 6.35;
    s.library.add(t1).unwrap();
    s.open_cli(&state::tests::bracket_path());
    let (outer, contour) = s
        .project
        .items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            hc_project::Geom::Contour(c) if c.closed => Some((i.id, c.clone())),
            _ => None,
        })
        .max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs()))
        .unwrap();
    s.selection = [outer].into_iter().collect();
    let op = s.new_operation(NewKind::Contour, 1).unwrap();
    let mut o = s.project.operations[0].clone();
    o.tabs.enabled = true;
    o.depth = 6.0;
    s.update_op(o.clone());
    s.select_op(op);
    let (pos, note) = hc_toolpath::tab_positions(&contour, &o.tabs, outer, 6.35);
    assert_eq!(pos.len(), 4);
    assert_eq!(note, None);
    s.selected_tab = Some((outer, 1));

    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
    h.run();
    assert!(h.query_by_label("Reset to auto").is_some());
    assert!(h.state().state.toolpaths[0].1.is_ok());
    save(&mut h, "tabs");
}

/// M8 test 33. Manual check: readable in light and dark mode, coloured severity words, no tofu.
#[test]
#[ignore]
fn shot_issues() {
    for (name, theme) in [
        ("issues", egui::Theme::Dark),
        ("issues_light", egui::Theme::Light),
    ] {
        let (s, _) = state::tests::bracket_issues();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        assert!(h.query_by_label_contains("Issues (2)").is_some());
        save(&mut h, name);
    }
}

/// M9 test 18. Manual check: band as wide as the tool, pocket lighter than the through contour, tool circle at the
/// readout's position, no tofu, the panel fits at 1280 px.
#[test]
#[ignore]
fn shot_sim() {
    for (name, theme) in [
        ("sim", egui::Theme::Dark),
        ("sim_light", egui::Theme::Light),
    ] {
        let mut s = toolpaths_state();
        let inside = s.selected_op.unwrap();
        s.delete_op(inside);
        // through cut, so the contour band is darker than the 3 mm pocket; pocket first so both show at 60 %
        let mut o = s.project.operations[0].clone();
        o.depth = s.project.stock.thickness;
        s.update_op(o);
        let pocket = s.project.operations[1].id;
        s.move_op(pocket, state::MoveDir::Up);
        s.sim_view.open = true;
        s.refresh_toolpaths();
        s.sim_view.t = 0.6 * s.sim_total();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        assert!(h.query_by_label_contains("Total ").is_some());
        save(&mut h, name);
    }
}

/// Screen position of world point `w`, found by hovering a known canvas point (the canvas rect is not stored).
fn screen_of(h: &mut Harness<'_, App>, w: hc_geom::Point) -> egui::Pos2 {
    let probe = egui::pos2(640.0, 400.0);
    h.hover_at(probe);
    h.run();
    let s = &h.state().state;
    let c = s.cursor.expect("canvas hovered");
    let z = s.view.zoom;
    probe + egui::vec2(((w.x - c.x) as f32) * z, -((w.y - c.y) as f32) * z)
}

/// M10 test 23 plus the stock menu and a drawing preview. Manual check: marker and `Corner` label readable,
/// arrows at the current origin, rubber band dashed, no tofu, light and dark.
#[test]
#[ignore]
fn shot_snap() {
    for (suffix, theme) in [("", egui::Theme::Dark), ("_light", egui::Theme::Light)] {
        let mut s = AppState::new(None);
        s.open_cli(&state::tests::bracket_path());
        s.set_tool(state::CanvasTool::SetOrigin);
        let edge = state::tests::outer(&s);
        let idx = s.project.items.iter().position(|i| i.id == edge).unwrap();
        let corner = s
            .snap_cache
            .iter()
            .filter(|(i, _, k)| *i == idx && *k == crate::snap::SnapKind::Corner)
            .map(|(_, p, _)| *p)
            .min_by(|a, b| (a.x + a.y).total_cmp(&(b.x + b.y)))
            .unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        let at = screen_of(&mut h, corner) + egui::vec2(3.0, 0.0);
        h.hover_at(at);
        h.run();
        let snap = h
            .state()
            .state
            .snap_at(
                h.state().state.cursor.unwrap(),
                10.0 / h.state().state.view.zoom as f64,
            )
            .unwrap();
        assert_eq!(snap.kind, crate::snap::SnapKind::Corner);
        assert!(h
            .query_by_label("Set origin: click a corner, end or center · Esc cancels")
            .is_some());
        save(&mut h, &format!("snap{suffix}"));

        // pick it: the arrows move to the corner
        h.state_mut().state.canvas_click(snap.p, true);
        h.run();
        assert_eq!(
            h.state().state.project.stock.origin,
            hc_project::WorkOrigin::Point(corner)
        );
        save(&mut h, &format!("set_origin{suffix}"));

        h.get_by_label("Settings").click();
        h.run();
        h.get_by_label("Stock").hover();
        h.run();
        assert!(h.query_by_label("Pick on drawing").is_some());
        save(&mut h, &format!("stock_menu{suffix}"));
        h.key_press(egui::Key::Escape);
        h.run();

        // rectangle preview from the corner to a free point
        h.state_mut().state.set_tool(state::CanvasTool::Rect);
        h.state_mut().state.canvas_click(corner, true);
        let far = screen_of(&mut h, corner) + egui::vec2(120.0, -80.0);
        h.hover_at(far);
        h.run();
        assert!(h
            .query_by_label("Rectangle: click the opposite corner · Esc cancels")
            .is_some());
        save(&mut h, &format!("draw_preview{suffix}"));
    }
}
