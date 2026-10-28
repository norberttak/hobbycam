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
    crate::kittest::menu(&h, "Show toolpaths").click();
    h.run();
    assert!(!h.state().state.show_toolpaths);
    // close the menu so the canvas is unobstructed
    h.key_press(egui::Key::Escape);
    h.run();
    save(&mut h, "toolpaths_hidden");
}

/// M6 test 14. Manual check: 4 markers on straight edges, the selected one orange.
/// The M6 tabs project: an Outside op with 4 auto tabs on the Bracket, tab 2 selected.
fn tabs_state() -> AppState {
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
    s
}

#[test]
#[ignore]
fn shot_tabs() {
    let s = tabs_state();
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

/// The M9 simulation state: Pocket, through Outside contour and Drill, 60 % into the job.
fn sim_state() -> AppState {
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
    s
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
        let s = sim_state();
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
        let names = ["snap", "set_origin", "settings", "draw_preview"]
            .map(|n| Some(format!("{n}{suffix}")));
        snap_series(
            theme,
            names,
            bottom_left_corner,
            crate::snap::SnapKind::Corner,
        );
    }
}

/// The bottom-left `Corner` snap of the Bracket's outer contour.
fn bottom_left_corner(s: &AppState) -> hc_geom::Point {
    let edge = state::tests::outer(s);
    let idx = s.project.items.iter().position(|i| i.id == edge).unwrap();
    s.snap_cache
        .cands
        .iter()
        .filter(|(i, _, _, k)| *i == idx && *k == crate::snap::SnapKind::Corner)
        .map(|(_, _, p, _)| *p)
        .min_by(|a, b| (a.x + a.y).total_cmp(&(b.x + b.y)))
        .unwrap()
}

/// Set origin hover over `target(s)` (a `kind` snap), the picked origin, Settings → Stock, a rectangle preview from
/// that point; each saved under its name if given.
fn snap_series(
    theme: egui::Theme,
    names: [Option<String>; 4],
    target: fn(&AppState) -> hc_geom::Point,
    kind: crate::snap::SnapKind,
) {
    let shot = |h: &mut Harness<'_, App>, k: usize| {
        if let Some(n) = &names[k] {
            save(h, n);
        }
    };
    {
        let mut s = AppState::new(None);
        no_config_error(&mut s);
        s.open_cli(&state::tests::bracket_path());
        s.set_tool(state::CanvasTool::SetOrigin);
        let corner = target(&s);
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
        assert_eq!(snap.kind, kind);
        assert!(h
            .query_by_label("Set origin: click a snap point · Esc cancels")
            .is_some());
        shot(&mut h, 0);

        // pick it: the arrows move to the corner
        h.state_mut().state.canvas_click(snap.p, true);
        h.run();
        assert_eq!(
            h.state().state.project.stock.origin,
            hc_project::WorkOrigin::Point(corner)
        );
        shot(&mut h, 1);

        h.state_mut().settings_open = true;
        h.run();
        assert!(h.query_by_label("Pick on drawing").is_some());
        shot(&mut h, 2);
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
        shot(&mut h, 3);
    }
}

/// The Bracket saved and re-opened, so the undo history is empty (Undo disabled).
fn clean_bracket(name: &str) -> AppState {
    let path = state::tests::scratch(name).join("bracket.hcam");
    let mut s = AppState::new(None);
    s.open_cli(&state::tests::bracket_path());
    assert!(s.save_as(&path));
    let mut s = AppState::new(None);
    assert!(s.open(&path));
    s
}

/// M10a.2 test 15. Manual check: white fill icons on group-coloured tiles, selected tile has the dark ring and the
/// arrow, disabled tiles grey, tooltip readable, nothing clipped, both themes.
#[test]
#[ignore]
fn shot_sidebar() {
    for (name, theme) in [
        ("sidebar_dark", egui::Theme::Dark),
        ("sidebar_light", egui::Theme::Light),
    ] {
        let mut s = clean_bracket(name);
        let edge = state::tests::outer(&s);
        s.selection = [edge].into_iter().collect();
        s.set_tool(state::CanvasTool::Line);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        assert!(!h.state().state.can_undo());
        crate::kittest::side(&h, "Line").hover();
        for _ in 0..60 {
            h.step(); // tooltip delay
        }
        h.run();
        save(&mut h, name);
    }
}

/// M10a.2 test 15: one hover per new marker (Center circle, Intersection X, Midpoint triangle).
#[test]
#[ignore]
fn shot_snap_kinds() {
    use crate::snap::SnapKind;
    for (suffix, theme) in [("", egui::Theme::Dark), ("_light", egui::Theme::Light)] {
        let mut s = clean_bracket(&format!("snap_kinds{suffix}"));
        s.set_tool(state::CanvasTool::Line);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        for kind in [SnapKind::Center, SnapKind::Intersection, SnapKind::Midpoint] {
            let st = &h.state().state;
            let at = st
                .snap_cache
                .cands
                .iter()
                .filter(|c| c.3 == kind)
                .map(|c| c.2)
                .find(|&q| st.snap_at(q, 1e-6).map(|sn| sn.kind) == Some(kind))
                .unwrap();
            let pos = screen_of(&mut h, at) + egui::vec2(2.0, 1.0);
            h.hover_at(pos);
            h.run();
            let st = &h.state().state;
            let r = 10.0 / st.view.zoom as f64;
            assert_eq!(
                st.snap_at(st.cursor.unwrap(), r).map(|s| s.kind),
                Some(kind)
            );
            save(
                &mut h,
                &format!("snap_{}{suffix}", kind.label().to_lowercase()),
            );
        }
    }
}

/// M10b test 31. Manual check: letters upright, not mirrored, baseline straight; Hershey on top, Ubuntu Light below.
#[test]
#[ignore]
fn shot_text() {
    for (suffix, theme) in [("", egui::Theme::Dark), ("_light", egui::Theme::Light)] {
        let s = text_state();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_theme(theme)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        assert!(h.query_by_label("Draw text").is_some());
        save(&mut h, &format!("text{suffix}"));
    }
}

/// Hershey text above Ubuntu Light text, the Text tool active.
fn text_state() -> AppState {
    let mut s = AppState::new(None);
    for (text, font, y) in [
        ("HobbyCAM AB gq 0123", hc_text::Font::Hershey, 20.0),
        ("HobbyCAM AB gq 0123", hc_text::Font::Outline, 0.0),
    ] {
        s.add_shape(state::Shape::Text {
            at: hc_geom::Point::new(0.0, y),
            text: text.into(),
            font,
            height: 10.0,
        })
        .unwrap();
    }
    s.clear_selection();
    s.set_tool(state::CanvasTool::Text);
    s.fit_pending = true;
    s
}

/// Tests have no config dir; drop that load error so pictures show what a user sees.
fn no_config_error(s: &mut AppState) {
    s.library_error = None;
    s.messages.retain(|m| !m.text.starts_with("Tool library:"));
}

/// Light-theme harness for the user manual pictures.
fn docs_harness(mut app: App) -> Harness<'static, App> {
    no_config_error(&mut app.state);
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_theme(egui::Theme::Light)
        .build_ui_state(|ui, app: &mut App| app.show(ui), app)
}

/// M11: the 12 pictures of the user manual.
/// Run from the repository root: HOBBYCAM_SHOT_DIR=$PWD/docs/src/images cargo test -p hobbycam shot_docs -- --ignored
#[test]
#[ignore]
fn shot_docs() {
    use state::tests::{bracket_clean, bracket_issues, edit_op};
    let run = |app: App, name: &str, check: &dyn Fn(&mut Harness<'_, App>)| {
        let mut h = docs_harness(app);
        h.run();
        check(&mut h);
        save(&mut h, name);
    };

    // walkthrough steps 2–3: real blank, origin at the Ø76.2 circle's Center
    let picked = state::tests::walkthrough_stock_origin;
    let (mut s, _) = bracket_clean();
    s.selected_op = None;
    let center = picked(&mut s);
    run(App::new(s), "main-window", &|h| {
        assert_eq!(
            h.state().state.project.stock.origin,
            hc_project::WorkOrigin::Point(center)
        );
        assert_eq!(h.state().state.project.operations.len(), 3);
        assert!(h.query_by_label("Drill 1 · T2").is_some());
    });

    let (mut s, _) = bracket_clean();
    for id in s
        .project
        .operations
        .iter()
        .map(|o| o.id)
        .collect::<Vec<_>>()
    {
        s.delete_op(id);
    }
    s.selection = [state::tests::outer(&s)].into_iter().collect();
    picked(&mut s);
    run(App::new(s), "walkthrough-new-op", &|h| {
        assert!(h.state().state.project.operations.is_empty());
        assert!(h.query_by_label("New operation").is_some());
    });

    let (mut s, _) = bracket_clean();
    s.selected_op = None;
    s.selection = [state::tests::circles(&s, 76.2)[0]].into_iter().collect();
    run(App::new(s), "edit-circle", &|h| {
        assert!(h.query_by_label("Diameter").is_some());
    });

    let (mut s, outside) = bracket_clean();
    s.select_op(outside);
    let center = picked(&mut s);
    run(App::new(s), "walkthrough-toolpaths", &|h| {
        let st = &h.state().state;
        assert_eq!(
            st.project.stock.origin,
            hc_project::WorkOrigin::Point(center)
        );
        assert_eq!(st.toolpaths.iter().filter(|(_, r)| r.is_ok()).count(), 3);
        assert!(st.issues.is_empty());
    });

    run(App::new(tabs_state()), "tabs", &|h| {
        assert!(h.query_by_label("Reset to auto").is_some());
    });

    run(App::new(bracket_issues().0), "issues", &|h| {
        assert!(h.query_by_label_contains("Issues (2)").is_some());
    });

    let (mut s, outside) = bracket_clean();
    edit_op(&mut s, outside, |o| o.rpm = 0);
    picked(&mut s);
    s.refresh_toolpaths();
    let mut app = App::new(s);
    app.export_prompt = Some(hc_toolpath::Severity::Warning);
    run(app, "export-warning", &|h| {
        assert!(h.query_by_label("Export anyway?").is_some());
    });

    run(App::new(sim_state()), "simulation", &|h| {
        assert!(h.query_by_label_contains("Total ").is_some());
    });

    let (mut s, _) = bracket_clean();
    no_config_error(&mut s);
    picked(&mut s);
    let mut app = App::new(s);
    app.tool_dialog.open = true;
    let mut h = docs_harness(app);
    h.run();
    assert!(h.query_by_label("Tool library").is_some());
    // open T1 in the form so every field shows (the first "Edit" is the menu bar's)
    h.query_all_by_label("Edit")
        .find(|n| n.rect().min.y > 40.0)
        .unwrap()
        .click();
    h.run();
    assert!(h.query_by_label("Flute length").is_some());
    save(&mut h, "tool-library");

    run(App::new(text_state()), "text", &|h| {
        assert!(h.query_by_label("Draw text").is_some());
    });

    snap_series(
        egui::Theme::Light,
        [Some("set-origin"), None, Some("settings"), Some("drawing")].map(|n| n.map(String::from)),
        state::tests::pocket_center,
        crate::snap::SnapKind::Center,
    );
}
