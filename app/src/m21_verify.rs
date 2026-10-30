//! M21 acceptance tests: Layers / Operations tab strip.

use crate::panels::ops_dot;
use crate::state::tests::{bracket_clean, bracket_issues, edit_op, scratch};
use crate::state::AppState;
use crate::theme;

#[test]
fn s1_remembered_across_start_up() {
    let d = scratch("m21_prefs");
    let mut s = AppState::new(Some(d.join("tools.json")));
    let mut p = s.prefs;
    p.ops_open = false;
    s.set_prefs(p);
    assert!(!s.dirty && !s.can_undo());
    assert!(std::fs::read_to_string(d.join("prefs.json"))
        .unwrap()
        .contains("\"ops_open\": false"));
    let q = AppState::new(Some(d.join("tools.json"))).prefs;
    assert!(!q.ops_open && q.layers_open);
}

#[test]
fn s2_old_file_opens_both() {
    let d = scratch("m21_old");
    std::fs::write(
        d.join("prefs.json"),
        r#"{"format_version":1,"snap_objects":true,"snap_grid":false}"#,
    )
    .unwrap();
    let s = AppState::new(Some(d.join("tools.json")));
    assert!(s.prefs.layers_open && s.prefs.ops_open && !s.prefs.snap_grid);
    assert!(!s
        .messages
        .iter()
        .any(|m| m.text.starts_with("Preferences:")));
}

#[test]
fn s3_ops_dot() {
    let v = theme::visuals(false);
    let (mut s, outside) = bracket_clean();
    s.refresh_toolpaths();
    assert_eq!(ops_dot(&s, &v), None);
    edit_op(&mut s, outside, |o| o.rpm = 0);
    s.refresh_toolpaths();
    assert_eq!(ops_dot(&s, &v), Some(v.warn_fg_color));
    let (mut s, _) = bracket_issues();
    s.refresh_toolpaths();
    assert_eq!(ops_dot(&s, &v), Some(v.error_fg_color));
    let mut i = s.issues[0].clone();
    i.op = None;
    s.issues = vec![i];
    assert_eq!(ops_dot(&s, &v), None);
}

mod kittest {
    use super::*;
    use crate::panels::STRIP_W;
    use crate::App;
    use eframe::egui;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    fn make(s: AppState) -> Harness<'static, App> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        h
    }

    fn clean() -> Harness<'static, App> {
        let (mut s, _) = bracket_clean();
        s.library_error = None;
        make(s)
    }

    fn tab<'a>(h: &'a Harness<'_, App>, name: &'a str) -> egui_kittest::Node<'a> {
        let mut v: Vec<_> = h
            .query_all_by_label(name)
            .filter(|n| n.rect().width() <= STRIP_W + 0.5)
            .collect();
        assert_eq!(v.len(), 1, "tab {name}");
        v.pop().unwrap()
    }

    fn heading<'a>(h: &'a Harness<'_, App>, name: &'a str) -> Option<egui_kittest::Node<'a>> {
        h.query_all_by_label(name)
            .find(|n| n.rect().width() > STRIP_W + 0.5)
    }

    fn toggled(n: &egui_kittest::Node<'_>) -> bool {
        n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    }

    fn click(h: &mut Harness<'static, App>, name: &str) {
        tab(h, name).click();
        h.run();
        h.run();
    }

    fn has(h: &Harness<'_, App>, text: &str) -> bool {
        h.query_all_by_label_contains(text).next().is_some()
    }

    #[test]
    fn k1_default() {
        let h = clean();
        let (l, o) = (tab(&h, "Layers"), tab(&h, "Operations"));
        assert!(toggled(&l) && toggled(&o));
        for n in [&l, &o] {
            assert!((n.rect().width() - STRIP_W).abs() < 0.5);
            assert!(n.rect().min.x >= crate::kittest::side(&h, "Line").rect().max.x);
        }
        assert!(o.rect().min.y >= l.rect().max.y - 0.5);
        assert!(heading(&h, "Layers").is_some() && heading(&h, "Operations").is_some());
        assert!(has(&h, "Show layer ") && has(&h, "Contour 1 · T1"));
    }

    #[test]
    fn k2_toggle_layers() {
        let mut h = clean();
        let y0 = heading(&h, "Layers").unwrap().rect().min.y;
        let (undo, dirty) = (h.state().state.can_undo(), h.state().state.dirty);
        click(&mut h, "Layers");
        assert!(!toggled(&tab(&h, "Layers")));
        assert!(!has(&h, "Show layer "));
        let y = heading(&h, "Operations").unwrap().rect().min.y;
        assert!((y - y0).abs() <= 2.0);
        let st = &h.state().state;
        assert!(!st.prefs.layers_open && st.dirty == dirty && st.can_undo() == undo);
        click(&mut h, "Layers");
        assert!(toggled(&tab(&h, "Layers")) && has(&h, "Show layer "));
    }

    #[test]
    fn k3_toggle_operations() {
        let mut h = clean();
        click(&mut h, "Operations");
        assert!(!toggled(&tab(&h, "Operations")));
        assert!(!has(&h, "Enable ") && !has(&h, "New operation"));
        assert!(has(&h, "Show layer "));
        click(&mut h, "Operations");
        assert!(has(&h, "Contour 1 · T1"));
    }

    #[test]
    fn k4_k5_both_closed_canvas_wider_width_kept() {
        let mut h = clean();
        let r0 = h.state().canvas_rect;
        click(&mut h, "Layers");
        click(&mut h, "Operations");
        assert_eq!(h.query_all_by_label("Layers").count(), 1);
        assert_eq!(h.query_all_by_label("Operations").count(), 1);
        let r = h.state().canvas_rect;
        assert!(r.width() >= r0.width() + 200.0);
        assert!(r.min.x <= tab(&h, "Layers").rect().max.x + 2.0);
        click(&mut h, "Layers");
        click(&mut h, "Operations");
        let r1 = h.state().canvas_rect;
        assert!((r1.width() - r0.width()).abs() <= 1.0);
        assert!((r1.min.x - r0.min.x).abs() <= 1.0);
    }

    #[test]
    fn k6_properties_while_hidden() {
        let (mut s, outside) = bracket_clean();
        s.library_error = None;
        s.select_op(outside);
        let mut h = make(s);
        click(&mut h, "Operations");
        assert!(h.query_by_label("Properties").is_some());
        assert!(h.query_by_label("Name").is_some());
        assert_eq!(h.state().state.selected_op, Some(outside));
    }

    #[test]
    fn k7_issue_click_does_not_expand() {
        let (mut s, _) = bracket_issues();
        s.library_error = None;
        s.refresh_toolpaths();
        let mut h = make(s);
        click(&mut h, "Operations");
        let i = h.state().state.issues[0].clone();
        let text = h.state().state.issue_text(&i);
        h.get_by_label(&text).click();
        h.run();
        h.run();
        let st = &h.state().state;
        assert_eq!(st.selected_op, i.op);
        assert!(!st.prefs.ops_open);
        assert!(!toggled(&tab(&h, "Operations")));
        assert!(!has(&h, "Enable "));
    }

    #[test]
    fn k8_dot_follows_issues() {
        let (mut s, _) = bracket_issues();
        s.library_error = None;
        s.refresh_toolpaths();
        let mut h = make(s);
        let v = h.ctx.global_style().visuals.clone();
        let want = Some(v.error_fg_color);
        assert_eq!(ops_dot(&h.state().state, &v), want);
        click(&mut h, "Operations");
        assert_eq!(ops_dot(&h.state().state, &v), want);
    }

    #[test]
    fn k9_new_operation_form_survives() {
        let (mut s, _) = bracket_clean();
        s.library_error = None;
        s.selection = [crate::state::tests::outer(&s)].into_iter().collect();
        let mut h = make(s);
        h.get_by_label("New operation").click();
        h.run();
        assert!(has(&h, "Create"));
        click(&mut h, "Operations");
        assert!(!has(&h, "Create"));
        assert!(h.state().new_op.is_some());
        click(&mut h, "Operations");
        assert!(has(&h, "Create"));
    }

    #[test]
    fn k10_saved_through_the_strip() {
        let d = scratch("m21_k10");
        let mut s = AppState::new(Some(d.join("tools.json")));
        s.open_cli(&crate::state::tests::bracket_path());
        let mut h = make(s);
        click(&mut h, "Layers");
        assert!(std::fs::read_to_string(d.join("prefs.json"))
            .unwrap()
            .contains("\"layers_open\": false"));
    }

    // ---------------------------------------------------------------- independent verifier additions

    fn build(s: AppState, size: egui::Vec2, th: egui::Theme) -> Harness<'static, App> {
        let mut h = Harness::builder()
            .with_size(size)
            .with_theme(th)
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
        h.run();
        h
    }

    fn pane_right(h: &Harness<'_, App>) -> f32 {
        h.state().canvas_rect.min.x
    }

    fn set(h: &mut Harness<'static, App>, layers: bool, ops: bool) {
        if toggled(&tab(h, "Layers")) != layers {
            click(h, "Layers");
        }
        if toggled(&tab(h, "Operations")) != ops {
            click(h, "Operations");
        }
    }

    #[test]
    fn v1_four_states() {
        let mut h = clean();
        let strip_r = tab(&h, "Layers").rect().max.x;
        let both = pane_right(&h);
        let full = h.state().canvas_rect.width();
        let mut widths = vec![];
        for (l, o) in [(true, true), (true, false), (false, true), (false, false)] {
            set(&mut h, l, o);
            assert_eq!(toggled(&tab(&h, "Layers")), l);
            assert_eq!(toggled(&tab(&h, "Operations")), o);
            assert_eq!(heading(&h, "Layers").is_some(), l, "{l} {o}");
            assert_eq!(heading(&h, "Operations").is_some(), o, "{l} {o}");
            assert_eq!(has(&h, "Show layer "), l);
            assert_eq!(has(&h, "Enable "), o);
            let (p, c) = (pane_right(&h), h.state().canvas_rect.width());
            if l || o {
                // the pane is content-sized, so only both-open must equal the default
                assert!(p > strip_r + 100.0 && c < full + 200.0, "{l} {o}: {p} {c}");
                if l && o {
                    assert!((p - both).abs() <= 1.0 && (c - full).abs() <= 1.0);
                }
            } else {
                assert!(p <= strip_r + 2.0 && c > full + 200.0);
            }
            widths.push(p);
        }
        assert!(widths[0] > strip_r + 100.0);
    }

    #[test]
    fn v2_dragged_width_restored() {
        let mut h = clean();
        let start = pane_right(&h);
        // drag the pane's right edge 80 px to the right
        let y = 400.0;
        h.drag_at(egui::pos2(start - 2.0, y));
        h.run();
        for dx in [20.0, 50.0, 78.0] {
            h.hover_at(egui::pos2(start - 2.0 + dx, y));
            h.run();
        }
        h.drop_at(egui::pos2(start + 78.0, y));
        h.run();
        h.run();
        let dragged = pane_right(&h);
        assert!(
            dragged > start + 40.0,
            "drag did not widen: {start} -> {dragged}"
        );
        for _ in 0..5 {
            h.run();
        }
        assert!((pane_right(&h) - dragged).abs() <= 1.0, "idle drift");
        set(&mut h, true, false);
        set(&mut h, true, true);
        set(&mut h, false, false);
        set(&mut h, true, true);
        assert!(
            (pane_right(&h) - dragged).abs() <= 1.0,
            "after both closed: {} vs {dragged}",
            pane_right(&h)
        );
        // also through a one-section state
        set(&mut h, false, true);
        set(&mut h, true, true);
        assert!((pane_right(&h) - dragged).abs() <= 1.0);
    }

    #[test]
    fn v3_keyboard_tab_reaches_and_space_enter_toggle() {
        for key in [egui::Key::Space, egui::Key::Enter] {
            let mut h = clean();
            // Tab from the last sidebar tile lands on the first strip tab, then the next one
            crate::kittest::side(&h, "Settings…").focus();
            h.run();
            h.key_press(egui::Key::Tab);
            h.run();
            assert!(
                tab(&h, "Layers").accesskit_node().is_focused(),
                "Tab -> Layers"
            );
            h.key_press(egui::Key::Tab);
            h.run();
            assert!(
                tab(&h, "Operations").accesskit_node().is_focused(),
                "Tab -> Operations"
            );
            h.key_press(key);
            h.run();
            h.run();
            assert!(!toggled(&tab(&h, "Operations")), "{key:?}");
            assert!(!h.state().state.prefs.ops_open);
            h.key_press(key);
            h.run();
            h.run();
            assert!(toggled(&tab(&h, "Operations")) && h.state().state.prefs.ops_open);
        }
    }

    fn px(h: &mut Harness<'static, App>, x: f32, y: f32) -> egui::Color32 {
        let p = *h.render().unwrap().get_pixel(x as u32, y as u32);
        egui::Color32::from_rgb(p[0], p[1], p[2])
    }

    #[test]
    fn v4_focus_ring_is_painted() {
        let mut h = clean();
        click(&mut h, "Layers"); // closed: no fill, no accent bar
        h.hover_at(egui::pos2(700.0, 400.0));
        h.run();
        let r = tab(&h, "Layers").rect();
        let probe = (r.max.x - 2.0, r.center().y); // the separator covers the last pixel
        let ring = h.ctx.global_style().visuals.hyperlink_color;
        let before = px(&mut h, probe.0, probe.1);
        tab(&h, "Layers").focus();
        h.run();
        let after = px(&mut h, probe.0, probe.1);
        assert_ne!(before, ring);
        assert_eq!(after, ring, "ring not painted at {probe:?}");
    }

    #[test]
    fn v5_dot_painted_open_and_closed_and_follows_issues() {
        let (mut s, outside) = bracket_clean();
        s.library_error = None;
        s.refresh_toolpaths();
        let mut h = make(s);
        let v = h.ctx.global_style().visuals.clone();
        fn dot_at(h: &mut Harness<'static, App>) -> egui::Color32 {
            h.hover_at(egui::pos2(700.0, 400.0));
            h.run();
            let r = tab(h, "Operations").rect();
            px(h, r.center().x, r.max.y - 8.0)
        }
        assert_eq!(ops_dot(&h.state().state, &v), None);
        let none = dot_at(&mut h);
        assert_ne!(none, v.warn_fg_color);
        assert_ne!(none, v.error_fg_color);
        // a problem appears: rpm 0 gives a warning
        edit_op(&mut h.state_mut().state, outside, |o| o.rpm = 0);
        h.state_mut().state.refresh_toolpaths();
        h.run();
        h.run();
        assert_eq!(dot_at(&mut h), v.warn_fg_color);
        click(&mut h, "Operations");
        assert_eq!(
            dot_at(&mut h),
            v.warn_fg_color,
            "closed tab still shows the dot"
        );
        click(&mut h, "Operations");
        // error: tool far too big
        let i0 = h.state().state.issues[0].clone();
        h.state_mut().state.issues.push(i0);
        h.state_mut().state.issues[0].severity = hc_toolpath::Severity::Error;
        h.run();
        h.run();
        assert_eq!(dot_at(&mut h), v.error_fg_color);
        // fixed
        h.state_mut().state.issues.clear();
        h.run();
        h.run();
        assert_eq!(dot_at(&mut h), none);
        // the tab never changes size with the dot
        let r1 = tab(&h, "Operations").rect();
        edit_op(&mut h.state_mut().state, outside, |o| o.rpm = 0);
        h.state_mut().state.refresh_toolpaths();
        h.run();
        h.run();
        assert_eq!(tab(&h, "Operations").rect(), r1);
    }

    #[test]
    fn v6_new_op_with_problem_then_fixed_updates_dot() {
        let (mut s, _) = bracket_clean();
        s.library_error = None;
        let v = theme::visuals(false);
        s.selection = [crate::state::tests::outer(&s)].into_iter().collect();
        let id = s.new_operation(crate::state::NewKind::Engrave, 1).unwrap();
        s.refresh_toolpaths();
        assert_eq!(ops_dot(&s, &v), None, "{:?}", s.issues);
        edit_op(&mut s, id, |o| o.rpm = 0);
        s.refresh_toolpaths();
        assert!(ops_dot(&s, &v).is_some());
        edit_op(&mut s, id, |o| o.rpm = 8000);
        s.refresh_toolpaths();
        assert_eq!(ops_dot(&s, &v), None);
    }

    #[test]
    fn v7_layer_visibility_survives_hide_show() {
        let (mut s, _) = bracket_clean();
        s.library_error = None;
        let name = s.project.layers[0].name.clone();
        s.set_layer_visible(&name, false);
        let mut h = make(s);
        click(&mut h, "Layers");
        click(&mut h, "Layers");
        let label = format!("Show layer {name}");
        let n = h.get_by_label(&label);
        assert_ne!(
            n.accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True)
        );
        assert!(h.state().state.hidden_layers.contains(&name));
    }

    #[test]
    fn v8_tools_unaffected_while_closed() {
        let mut h = clean();
        set(&mut h, false, false);
        crate::kittest::side(&h, "Line").click();
        h.run();
        assert_eq!(h.state().state.tool, crate::state::CanvasTool::Line);
        crate::kittest::side(&h, "Measure").click();
        h.run();
        assert_eq!(h.state().state.tool, crate::state::CanvasTool::Measure);
        // the canvas starts right of the strip and receives clicks there
        let c = h.state().canvas_rect;
        assert!(c.min.x >= tab(&h, "Layers").rect().max.x - 1.0);
        let n = h.state().state.project.items.len();
        crate::kittest::side(&h, "Select").click();
        h.run();
        h.hover_at(egui::pos2(c.min.x + 5.0, c.center().y));
        h.run();
        assert_eq!(h.state().state.project.items.len(), n);
    }

    #[test]
    fn v9_zoom_to_fit_uses_new_width() {
        let size = egui::vec2(900.0, 800.0);
        let (mut s, _) = bracket_clean();
        s.library_error = None;
        let mut h = build(s, size, egui::Theme::Light);
        crate::kittest::side(&h, "Zoom to fit").click();
        h.run();
        h.run();
        let z_open = h.state().state.view.zoom;
        set(&mut h, false, false);
        crate::kittest::side(&h, "Zoom to fit").click();
        h.run();
        h.run();
        let z_closed = h.state().state.view.zoom;
        assert!(
            z_closed > z_open * 1.05,
            "fit ignored the wider canvas: {z_open} -> {z_closed}"
        );
    }

    #[test]
    fn v10_failed_save_warns_and_no_real_config_dir() {
        let d = scratch("m21_v10");
        let mut s = AppState::new(Some(d.join("tools.json")));
        // a directory where prefs.json should be makes the save fail
        std::fs::create_dir_all(d.join("prefs.json")).unwrap();
        let mut p = s.prefs;
        p.layers_open = false;
        s.set_prefs(p);
        assert!(!s.prefs.layers_open, "state still follows the click");
        assert!(s
            .messages
            .iter()
            .any(|m| m.text.starts_with("Cannot save preferences")));
        assert!(!s.dirty && !s.can_undo());
        // tests above use AppState::new(None) or a scratch dir: no path under the user config dir
        assert!(AppState::new(None).prefs_path.is_none());
    }

    /// Manual check (run with HOBBYCAM_SHOT_DIR): strip in both themes, dot, focus ring.
    #[test]
    #[ignore]
    fn v_shot_strip() {
        let dir = std::env::var("HOBBYCAM_SHOT_DIR").expect("HOBBYCAM_SHOT_DIR");
        for (n, th) in [
            ("m21_dark", egui::Theme::Dark),
            ("m21_light", egui::Theme::Light),
        ] {
            let (mut s, _) = bracket_issues();
            s.library_error = None;
            s.refresh_toolpaths();
            let mut p = s.prefs;
            p.layers_open = false;
            s.prefs = p;
            let mut h = build(s, egui::vec2(380.0, 560.0), th);
            h.state_mut().state.prefs.layers_open = false;
            h.run();
            h.hover_at(tab(&h, "Operations").rect().center());
            h.run();
            let img = h.render().unwrap();
            img.save(format!("{dir}/{n}.png")).unwrap();
        }
    }
}
