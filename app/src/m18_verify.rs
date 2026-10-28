//! M18 verifier tests: sidebar tiles (click / disabled / selected / keyboard), ring geometry, fit, theme.

use crate::state::{self, AppState, CanvasTool};
use crate::{theme, App};
use eframe::egui::{self, Rect};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

const NAMES: [&str; 23] = [
    "New",
    "Open project…",
    "Save",
    "Import DXF…",
    "Export G-code…",
    "Undo",
    "Redo",
    "Delete selection",
    "Rotate selection",
    "Flip selection horizontally",
    "Flip selection vertically",
    "Select",
    "Line",
    "Rectangle",
    "Circle",
    "Text",
    "Round corners",
    "Set origin",
    "Zoom to fit",
    "Show toolpaths",
    "Simulation",
    "Tool library…",
    "Settings…",
];

const TOOLS: [(&str, CanvasTool); 7] = [
    ("Select", CanvasTool::Select),
    ("Line", CanvasTool::Line),
    ("Rectangle", CanvasTool::Rect),
    ("Circle", CanvasTool::Circle),
    ("Text", CanvasTool::Text),
    ("Round corners", CanvasTool::Fillet),
    ("Set origin", CanvasTool::SetOrigin),
];

fn harness_sized(s: AppState, size: egui::Vec2, th: Option<egui::Theme>) -> Harness<'static, App> {
    let mut b = Harness::builder().with_size(size);
    if let Some(t) = th {
        b = b.with_theme(t);
    }
    b.build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
}

fn bracket() -> Harness<'static, App> {
    let (s, _) = state::tests::bracket_clean();
    harness_sized(s, egui::vec2(1280.0, 800.0), None)
}

fn rect(h: &Harness<'_, App>, name: &str) -> Rect {
    crate::kittest::side(h, name).rect()
}

fn on(h: &Harness<'_, App>, name: &str) -> bool {
    crate::kittest::side(h, name).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

fn disabled(h: &Harness<'_, App>, name: &str) -> bool {
    crate::kittest::side(h, name).accesskit_node().is_disabled()
}

fn click(h: &mut Harness<'static, App>, name: &str) {
    h.hover_at(egui::pos2(700.0, 400.0));
    h.run();
    crate::kittest::side(h, name).click();
    h.run();
}

#[test]
fn m18v_tool_tiles_click_and_selected_state() {
    let mut h = bracket();
    h.run();
    for (name, t) in TOOLS {
        click(&mut h, name);
        assert_eq!(h.state().state.tool, t, "{name}");
        for (other, _) in TOOLS {
            assert_eq!(on(&h, other), other == name, "{name} clicked, {other}");
        }
    }
}

#[test]
fn m18v_toggle_tiles_follow_state_both_selected() {
    let mut h = bracket();
    h.run();
    let st = &h.state().state;
    let (tp0, sim0) = (st.show_toolpaths, st.sim_view.open);
    assert_eq!(
        (on(&h, "Show toolpaths"), on(&h, "Simulation")),
        (tp0, sim0)
    );
    if !tp0 {
        click(&mut h, "Show toolpaths");
    }
    if !h.state().state.sim_view.open {
        click(&mut h, "Simulation");
    }
    assert!(on(&h, "Show toolpaths") && on(&h, "Simulation"));
    click(&mut h, "Show toolpaths");
    assert!(!h.state().state.show_toolpaths && !on(&h, "Show toolpaths") && on(&h, "Simulation"));
    click(&mut h, "Simulation");
    assert!(!h.state().state.sim_view.open && !on(&h, "Simulation"));
}

#[test]
fn m18v_other_tiles_click_works_and_are_not_toggles() {
    let mut h = bracket();
    h.run();
    for n in [
        "Settings…",
        "Tool library…",
        "Zoom to fit",
        "Undo",
        "Delete selection",
    ] {
        assert!(!on(&h, n), "{n} must not look selected");
    }
    click(&mut h, "Settings…");
    assert!(h.state().settings_open);
    click(&mut h, "Tool library…");
    assert!(h.state().tool_dialog.open);
}

#[test]
fn m18v_edit_tiles_enabled_work_disabled_do_nothing() {
    let mut h = bracket();
    h.run();
    let snap = |h: &Harness<'_, App>| {
        let s = &h.state().state;
        (format!("{:?}", s.project.items), s.can_undo(), s.can_redo())
    };
    h.state_mut().state.selection.clear();
    h.run();
    for (n, want) in [
        ("Undo", !h.state().state.can_undo()),
        ("Redo", !h.state().state.can_redo()),
    ] {
        assert_eq!(disabled(&h, n), want, "{n}");
    }
    for n in [
        "Delete selection",
        "Rotate selection",
        "Flip selection horizontally",
        "Flip selection vertically",
    ] {
        assert!(disabled(&h, n), "{n}");
        let before = snap(&h);
        click(&mut h, n);
        assert_eq!(snap(&h), before, "{n} disabled click changed state");
    }
    let first = h.state().state.project.items[0].id;
    h.state_mut().state.selection = [first].into_iter().collect();
    h.run();
    for n in [
        "Rotate selection",
        "Flip selection horizontally",
        "Flip selection vertically",
    ] {
        assert!(!disabled(&h, n), "{n}");
        let before = snap(&h);
        click(&mut h, n);
        assert_ne!(snap(&h).0, before.0, "{n} enabled click did nothing");
    }
    assert!(!disabled(&h, "Undo") && disabled(&h, "Redo"));
    let before = format!("{:?}", h.state().state.project.items);
    click(&mut h, "Undo");
    assert_ne!(format!("{:?}", h.state().state.project.items), before);
    assert!(!disabled(&h, "Redo"));
    click(&mut h, "Redo");
    click(&mut h, "Delete selection");
    assert!(h.state().state.selection.is_empty());
    assert!(disabled(&h, "Delete selection"));
}

#[test]
fn m18v_zoom_to_fit_click() {
    let mut h = bracket();
    h.run();
    h.state_mut().state.view.zoom *= 0.2;
    let before = h.state().state.view.zoom;
    click(&mut h, "Zoom to fit");
    h.run();
    assert_ne!(h.state().state.view.zoom, before);
}

#[test]
fn m18v_keyboard_activates_tile() {
    for key in [egui::Key::Space, egui::Key::Enter] {
        let mut h = bracket();
        h.run();
        crate::kittest::side(&h, "Circle").focus();
        h.run();
        h.key_press(key);
        h.run();
        assert_eq!(h.state().state.tool, CanvasTool::Circle, "{key:?}");
        h.state_mut().state.selection.clear();
        h.run();
        let n = h.state().state.project.items.len();
        crate::kittest::side(&h, "Delete selection").focus();
        h.key_press(key);
        h.run();
        assert_eq!(h.state().state.project.items.len(), n);
    }
}

#[test]
fn m18v_hover_and_press_every_tile_no_panic() {
    let away = egui::pos2(700.0, 400.0);
    for th in [egui::Theme::Light, egui::Theme::Dark] {
        let (s, _) = state::tests::bracket_clean();
        let mut h = harness_sized(s, egui::vec2(1280.0, 800.0), Some(th));
        h.run();
        for n in NAMES {
            let c = rect(&h, n).center();
            h.hover_at(c);
            h.run();
            let btn = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            h.event(btn(c, true));
            h.run();
            h.hover_at(away);
            h.event(btn(away, false));
            h.run();
        }
    }
}

/// (outer ring box, arrow box) of a selected tile, as painted by `side_widget`.
fn marks(r: Rect) -> (Rect, Rect) {
    let arrow = Rect::from_min_max(
        egui::pos2(r.left() - 11.0, r.center().y - 5.0),
        egui::pos2(r.left() - 6.0, r.center().y + 5.0),
    );
    (r.expand(5.0), arrow)
}

#[test]
fn m18v_ring_and_arrow_never_overlap_or_clip() {
    let (mut s, _) = state::tests::bracket_issues();
    s.show_toolpaths = true;
    s.sim_view.open = true;
    let mut h = harness_sized(s, egui::vec2(1280.0, 800.0), None);
    h.run();
    for (name, t) in TOOLS {
        h.state_mut().state.set_tool(t);
        h.run();
        let sel_names = [name, "Show toolpaths", "Simulation"];
        for n in sel_names {
            assert!(on(&h, n), "{n}");
        }
        let panel_left = rect(&h, "Select").min.x - 12.0;
        let panel_right = panel_left + crate::panels::SIDEBAR_W;
        let status_top = h.get_by_label("X – Y –").rect().min.y;
        let issues_top = h.get_by_label_contains("Issues (").rect().min.y;
        let bottom = status_top.min(issues_top);
        let menu_bottom = crate::kittest::menu_q(&h, "File", false)
            .unwrap()
            .rect()
            .max
            .y;
        for n in sel_names {
            let (ring, arrow) = marks(rect(&h, n));
            assert!(
                ring.min.x >= panel_left && ring.max.x <= panel_right,
                "{n} clipped x {ring:?}"
            );
            assert!(arrow.min.x >= panel_left, "{n} arrow clipped");
            assert!(
                ring.min.y >= menu_bottom && ring.max.y <= bottom,
                "{n} clipped y {ring:?} [{menu_bottom},{bottom}]"
            );
            for other in NAMES {
                if other == n {
                    continue;
                }
                let o = rect(&h, other);
                assert!(!ring.intersects(o), "{n} ring hits tile {other}");
                assert!(!arrow.intersects(o), "{n} arrow hits tile {other}");
                if sel_names.contains(&other) {
                    let (r2, a2) = marks(o);
                    assert!(
                        !ring.intersects(r2) && !arrow.intersects(r2) && !a2.intersects(ring),
                        "{n} marks hit {other} marks"
                    );
                }
            }
        }
    }
}

#[test]
fn m18v_sidebar_vertical_fit_report() {
    for (w, hgt, sim) in [
        (1280.0, 800.0, false),
        (1280.0, 720.0, false),
        (1280.0, 800.0, true),
    ] {
        let (mut s, _) = state::tests::bracket_issues();
        s.sim_view.open = sim;
        let mut h = harness_sized(s, egui::vec2(w, hgt), None);
        h.run();
        let status_top = h.get_by_label("X – Y –").rect().min.y;
        let issues_top = h.get_by_label_contains("Issues (").rect().min.y;
        let bottom = status_top.min(issues_top);
        let last = NAMES.iter().map(|n| rect(&h, n).max.y).fold(0.0, f32::max);
        eprintln!(
            "M18V fit {w}x{hgt} sim={sim}: last tile bottom {last}, panels top {bottom}, slack after ring {}",
            bottom - last - 5.0
        );
        if hgt == 800.0 {
            for n in NAMES {
                let r = rect(&h, n);
                assert!(
                    r.max.y + 6.0 <= bottom - 2.0,
                    "{n} sim={sim}: {r:?} {bottom}"
                );
            }
        }
    }
}

#[test]
fn m18v_theme_switches_both_ways() {
    let (s, _) = state::tests::bracket_clean();
    let mut h = harness_sized(s, egui::vec2(1280.0, 800.0), Some(egui::Theme::Dark));
    h.run();
    for (th, p) in [
        (egui::Theme::Light, &theme::LIGHT),
        (egui::Theme::Dark, &theme::DARK),
        (egui::Theme::Light, &theme::LIGHT),
    ] {
        h.ctx.set_theme(th);
        h.run();
        let v = h.ctx.global_style().visuals.clone();
        assert_eq!(v.dark_mode, th == egui::Theme::Dark);
        assert_eq!((v.panel_fill, v.extreme_bg_color), (p.panel, p.canvas));
        assert_eq!(v.text_edit_bg_color, Some(p.input));
        assert_eq!(v.window_fill, p.win);
    }
    assert_ne!(theme::canvas_palette(false), theme::canvas_palette(true));
}

/// WCAG contrast of white on every enabled tile colour and of the ring on the panel (printed, with a 3:1 floor for icons).
#[test]
fn m18v_contrast_report() {
    fn lum(c: egui::Color32) -> f64 {
        let f = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }
    let cr = |a, b| {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    use theme::Tint::*;
    for dark in [false, true] {
        for t in [Draw, View, Edit, File, Ops, Set] {
            let st = theme::tile_style(t, dark, true, true, false, false);
            let (gap, ring) = st.ring.unwrap();
            eprintln!(
                "M18V contrast dark={dark} {t:?}: icon/fill {:.2}, ring/panel {:.2}, ring/fill {:.2}",
                cr(st.icon, st.fill),
                cr(ring, gap),
                cr(ring, st.fill)
            );
        }
        let st = theme::tile_style(Edit, dark, false, false, false, false);
        eprintln!(
            "M18V contrast dark={dark} disabled icon/fill {:.2}",
            cr(st.icon, st.fill)
        );
    }
}

/// Full-window light/dark snapshots plus pixel checks of ring and gap. Needs wgpu: ignored like the other shots.
#[test]
#[ignore]
fn m18v_shots() {
    let dir = std::env::var("HOBBYCAM_SHOT_DIR").expect("set HOBBYCAM_SHOT_DIR");
    for (th, p, tag) in [
        (egui::Theme::Light, &theme::LIGHT, "light"),
        (egui::Theme::Dark, &theme::DARK, "dark"),
    ] {
        let (mut s, _) = state::tests::bracket_issues();
        s.show_toolpaths = true;
        s.sim_view.open = true;
        s.set_tool(CanvasTool::Line);
        let mut h = harness_sized(s, egui::vec2(1280.0, 800.0), Some(th));
        h.state_mut().tool_dialog.open = true;
        h.state_mut().settings_open = true;
        h.run();
        let img = h.render().expect("render");
        img.save(format!("{dir}/m18_{tag}_full.png")).unwrap();
        let ppp = h.ctx.pixels_per_point();
        let px = |x: f32, y: f32| *img.get_pixel((x * ppp) as u32, (y * ppp) as u32);
        let r = rect(&h, "Line");
        let q = px(r.left() - 3.75, r.center().y + 10.0);
        eprintln!("M18V {tag}: ring px {q:?} want {:?}; ppp {ppp}", p.head);
        assert_eq!(
            [q[0], q[1], q[2]],
            [p.head.r(), p.head.g(), p.head.b()],
            "{tag} ring"
        );
        let g = px(r.left() - 1.25, r.center().y + 10.0);
        assert_eq!(
            [g[0], g[1], g[2]],
            [p.panel.r(), p.panel.g(), p.panel.b()],
            "{tag} gap"
        );
    }
}
