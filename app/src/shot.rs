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

/// Outline circle ids with diameter `d` (mm).
fn circles(s: &AppState, d: f64) -> Vec<u64> {
    s.project
        .items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter(|i| match &i.geom {
            hc_project::Geom::Contour(c) => c
                .as_circle()
                .is_some_and(|ci| (2.0 * ci.radius - d).abs() < 1e-6),
            _ => false,
        })
        .map(|i| i.id)
        .collect()
}

/// M5 test 17. Manual check: feed vs rapid clearly different, arcs smooth, no path through islands.
#[test]
#[ignore]
fn shot_toolpaths() {
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
