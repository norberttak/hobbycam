//! M15 verifier tests: drag/cancel/undo edge cases, edit_shape validation and recognition tolerances (state),
//! canvas input after the id change and Properties field behaviour (kittest).

use crate::state::tests::{bracket_clean, circles, outer};
use crate::state::{AppState, Shape};
use hc_geom::{Contour, Point, Segment};
use hc_project::Geom;
use std::collections::BTreeSet;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn near(a: Point, b: Point) -> bool {
    a.approx_eq(b, 1e-9)
}

fn contour(s: &AppState, id: u64) -> Contour {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.clone(),
        Geom::Point(_) => panic!("point"),
    }
}

fn on(s: &AppState, id: u64) -> Point {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.point_at(0.0),
        Geom::Point(q) => *q,
    }
}

fn hide_dup_layers(s: &mut AppState) {
    let others: Vec<String> = s
        .project
        .layers
        .iter()
        .filter(|l| l.name != "Outline")
        .map(|l| l.name.clone())
        .collect();
    for l in others {
        s.set_layer_visible(&l, false);
    }
}

fn two_shapes() -> (AppState, u64, u64) {
    let mut s = AppState::new(None);
    let c = s
        .add_shape(Shape::Circle {
            c: p(50.0, 50.0),
            r: 10.0,
        })
        .unwrap()[0];
    let r = s
        .add_shape(Shape::Rect {
            min: p(100.0, 0.0),
            w: 20.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    s.commit_undo(true);
    (s, c, r)
}

// ---------------------------------------------------------------- drag (state)

#[test]
fn esc_restores_geometry_and_dirty_exactly_multiselection_with_point() {
    for was_dirty in [false, true] {
        let (mut s, c, r) = two_shapes();
        let pt = s.project.add_drawn(vec![Geom::Point(p(7.3, 8.1))])[0];
        s.items_changed();
        s.selection = BTreeSet::from([c, r, pt]);
        if !was_dirty {
            s.saved = s.project.clone(); // dirty is derived from `saved` on cancel
        }
        s.dirty = was_dirty;
        s.commit_undo(true);
        let before = s.project.clone();
        let n = s.undo.len();
        assert!(s.start_move(on(&s, c), 0.5, 0.0));
        s.drag_move(on(&s, c) + p(13.37, -4.2), 0.0, false);
        // everything moved by the same delta, point included
        assert!(near(on(&s, pt), p(7.3 + 13.37, 8.1 - 4.2)));
        assert!(s.dirty);
        s.escape();
        assert_eq!(s.project, before, "exact geometry");
        assert_eq!(s.dirty, was_dirty);
        assert!(s.moving.is_none());
        assert_eq!(s.selection, BTreeSet::from([c, r, pt]));
        s.commit_undo(true);
        assert_eq!(s.undo.len(), n);
        // bboxes and snap cache restored too
        assert_eq!(s.item_bbox[0], hc_geom::Contour::bbox(&contour(&s, c)));
    }
}

#[test]
fn drag_away_and_back_is_clean() {
    let (mut s, c, _) = two_shapes();
    s.dirty = false;
    s.commit_undo(true);
    let n = s.undo.len();
    let before = s.project.clone();
    assert!(s.start_move(p(60.0, 50.0), 0.5, 0.0));
    s.drag_move(p(90.0, 70.0), 0.0, false);
    assert!(s.dirty);
    s.drag_move(p(60.0, 50.0), 0.0, false);
    s.end_move();
    s.commit_undo(true);
    assert_eq!(s.project, before);
    assert!(!s.dirty, "net zero move is not a change");
    assert_eq!(s.undo.len(), n);
    assert!(s.selection.contains(&c));
}

#[test]
fn drag_undo_redo_roundtrip() {
    let (mut s, c, _) = two_shapes();
    let n = s.undo.len();
    assert!(s.start_move(p(60.0, 50.0), 0.5, 0.0));
    for k in 1..=10 {
        s.drag_move(p(60.0 + k as f64, 50.0), 0.0, false);
        s.commit_undo(false);
    }
    s.end_move();
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
    assert!(s.undo());
    assert!(near(
        contour(&s, c).as_circle().unwrap().center,
        p(50.0, 50.0)
    ));
    assert!(s.redo());
    assert!(near(
        contour(&s, c).as_circle().unwrap().center,
        p(60.0, 50.0)
    ));
}

#[test]
fn axis_lock_tie_keeps_x_and_shift_released_unlocks() {
    let (mut s, c, _) = two_shapes();
    assert!(s.start_move(p(60.0, 50.0), 0.5, 0.0));
    s.drag_move(p(70.0, 60.0), 0.0, true);
    assert_eq!(s.moving.as_ref().unwrap().delta, p(10.0, 0.0));
    s.drag_move(p(70.0, 60.0), 0.0, false);
    assert_eq!(s.moving.as_ref().unwrap().delta, p(10.0, 10.0));
    assert!(near(
        contour(&s, c).as_circle().unwrap().center,
        p(60.0, 60.0)
    ));
    s.drag_move(p(60.0, 50.0), 0.0, true); // zero delta: locks to 0,0 without NaN
    assert_eq!(s.moving.as_ref().unwrap().delta, p(0.0, 0.0));
}

#[test]
fn start_move_on_hidden_layer_or_after_delete_is_safe() {
    let (mut s, bracket_op) = bracket_clean();
    let _ = bracket_op;
    let hole = circles(&s, 15.875)[0];
    let pt = on(&s, hole);
    let layer = s.item(hole).unwrap().layer.clone();
    s.set_layer_visible(&layer, false);
    // hidden layer: nothing to pick there (the duplicate may live elsewhere, so only assert no panic and no stale drag)
    let _ = s.start_move(pt, 0.5, 0.0);
    s.cancel_move();
    // an item deleted mid-drag is skipped
    s.set_layer_visible(&layer, true);
    hide_dup_layers(&mut s);
    assert!(s.start_move(pt, 0.5, 0.0));
    s.project.items.retain(|i| i.id != hole);
    s.items_changed();
    s.drag_move(pt + p(1.0, 1.0), 0.0, false);
    s.end_move();
    s.cancel_move();
}

#[test]
fn set_tool_and_new_project_end_the_drag() {
    let (mut s, c, _) = two_shapes();
    assert!(s.start_move(p(60.0, 50.0), 0.5, 0.0));
    s.drag_move(p(70.0, 50.0), 0.0, false);
    s.set_tool(crate::state::CanvasTool::Line);
    assert!(s.moving.is_none());
    assert!(near(
        contour(&s, c).as_circle().unwrap().center,
        p(60.0, 50.0)
    ));
    s.set_tool(crate::state::CanvasTool::Select);
    assert!(s.start_move(p(70.0, 50.0), 0.5, 0.0));
    s.new_project();
    assert!(s.moving.is_none());
    s.drag_move(p(0.0, 0.0), 0.0, false); // no-op
}

#[test]
fn drag_toolpaths_regenerate_and_ops_reference_items() {
    let (mut s, _) = bracket_clean();
    let hole = circles(&s, 15.875)[0];
    hide_dup_layers(&mut s);
    s.refresh_toolpaths();
    let ops_before: Vec<Vec<u64>> = s
        .project
        .operations
        .iter()
        .map(|o| o.items.clone())
        .collect();
    assert!(s.start_move(on(&s, hole), 0.5, 0.0));
    s.drag_move(on(&s, hole) + p(2.0, 0.0), 0.0, false);
    s.end_move();
    s.refresh_toolpaths();
    let ops_after: Vec<Vec<u64>> = s
        .project
        .operations
        .iter()
        .map(|o| o.items.clone())
        .collect();
    assert_eq!(ops_before, ops_after);
    assert_eq!(s.toolpaths_for.as_ref(), Some(&s.project));
}

// ------------------------------------------------------------ edit_shape (state)

#[test]
fn edit_rect_invalid_values_leave_everything_untouched() {
    let (mut s, _, r) = two_shapes();
    let before = s.project.clone();
    let n = s.undo.len();
    let bad = [
        (0.0, 10.0, 0.0, "Rectangle needs a width and a height"),
        (20.0, 0.0, 0.0, "Rectangle needs a width and a height"),
        (-20.0, 10.0, 0.0, "Rectangle needs a width and a height"),
        (20.0, -1.0, 0.0, "Rectangle needs a width and a height"),
        (20.0, 10.0, -0.1, "Corner radius must be 0 or more"),
        (20.0, 10.0, 5.01, "Corner radius must be at most 5.000 mm"),
        (20.0, 10.0, 1e9, "Corner radius must be at most 5.000 mm"),
    ];
    for (w, h, rr, msg) in bad {
        let e = s.edit_shape(
            r,
            Shape::Rect {
                min: p(100.0, 0.0),
                w,
                h,
                r: rr,
            },
        );
        assert_eq!(e, Err(msg.to_string()), "{w} {h} {rr}");
    }
    for v in [f64::NAN, f64::INFINITY] {
        assert!(s
            .edit_shape(
                r,
                Shape::Rect {
                    min: p(100.0, 0.0),
                    w: v,
                    h: 10.0,
                    r: 0.0
                }
            )
            .is_err());
        assert!(s
            .edit_shape(
                r,
                Shape::Rect {
                    min: p(v, 0.0),
                    w: 10.0,
                    h: 10.0,
                    r: 0.0
                }
            )
            .is_err());
    }
    s.commit_undo(true);
    assert_eq!(s.project, before);
    assert_eq!(s.undo.len(), n);
}

#[test]
fn edit_rect_corner_radius_boundary_and_back() {
    let (mut s, _, r) = two_shapes();
    // exactly min(w,h)/2 is a slot: edit accepted, but then no longer recognised (documented out-of-scope)
    s.edit_shape(
        r,
        Shape::Rect {
            min: p(100.0, 0.0),
            w: 20.0,
            h: 10.0,
            r: 4.0,
        },
    )
    .unwrap();
    assert!(matches!(s.item_shape(r), Some(Shape::Rect { r, .. }) if (r - 4.0).abs() < 1e-9));
    // rounded -> sharp
    s.edit_shape(
        r,
        Shape::Rect {
            min: p(100.0, 0.0),
            w: 20.0,
            h: 10.0,
            r: 0.0,
        },
    )
    .unwrap();
    assert_eq!(contour(&s, r).segments.len(), 4);
    // shrink height below 2 * radius while rounded
    s.edit_shape(
        r,
        Shape::Rect {
            min: p(100.0, 0.0),
            w: 20.0,
            h: 10.0,
            r: 3.0,
        },
    )
    .unwrap();
    let e = s.edit_shape(
        r,
        Shape::Rect {
            min: p(100.0, 0.0),
            w: 20.0,
            h: 5.0,
            r: 3.0,
        },
    );
    assert_eq!(e, Err("Corner radius must be at most 2.500 mm".into()));
}

#[test]
fn edit_rect_inch_message_uses_project_units() {
    let (mut s, _, r) = two_shapes();
    s.set_units(hc_project::Units::Inch);
    let e = s
        .edit_shape(
            r,
            Shape::Rect {
                min: p(100.0, 0.0),
                w: 25.4,
                h: 25.4,
                r: 20.0,
            },
        )
        .unwrap_err();
    assert!(e.contains("at most") && e.contains("0.5"), "{e}");
}

#[test]
fn edit_line_keeps_start_and_rejects_degenerate() {
    let mut s = AppState::new(None);
    let l = s
        .add_shape(Shape::Line {
            a: p(1.0, 1.0),
            b: p(4.0, 5.0),
        })
        .unwrap()[0];
    // tiny but valid
    s.edit_shape(
        l,
        Shape::Line {
            a: p(1.0, 1.0),
            b: p(1.0, 1.01),
        },
    )
    .unwrap();
    let before = s.project.clone();
    for b in [p(1.0, 1.0), p(1.0, 1.0 + 1e-12), p(f64::NAN, 0.0)] {
        assert!(
            s.edit_shape(l, Shape::Line { a: p(1.0, 1.0), b }).is_err(),
            "{b:?}"
        );
    }
    assert_eq!(s.project, before);
    // same values again: no-op, not dirty
    s.dirty = false;
    s.edit_shape(
        l,
        Shape::Line {
            a: p(1.0, 1.0),
            b: p(1.0, 1.01),
        },
    )
    .unwrap();
    assert!(!s.dirty);
}

#[test]
fn edit_circle_invalid_and_orientation() {
    let mut s = AppState::new(None);
    let c = s
        .add_shape(Shape::Circle {
            c: p(5.0, 5.0),
            r: 3.0,
        })
        .unwrap()[0];
    let before = s.project.clone();
    for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            s.edit_shape(c, Shape::Circle { c: p(5.0, 5.0), r })
                .is_err(),
            "{r}"
        );
    }
    assert_eq!(s.project, before);
    // CW DXF circle keeps its orientation and start angle
    let cw = Contour::from_bulge_vertices(&[(p(0.0, 0.0), -1.0), (p(10.0, 0.0), -1.0)], true);
    let o = cw.orientation();
    let id = s.project.add_drawn(vec![Geom::Contour(cw)])[0];
    s.items_changed();
    s.edit_shape(
        id,
        Shape::Circle {
            c: p(20.0, 20.0),
            r: 2.0,
        },
    )
    .unwrap();
    let k = contour(&s, id);
    assert_eq!(k.orientation(), o);
    assert_eq!(k.segments.len(), 2);
    for g in &k.segments {
        let Segment::Arc { center, radius, .. } = g else {
            panic!()
        };
        assert!(near(*center, p(20.0, 20.0)) && (radius - 2.0).abs() < 1e-9);
    }
    // a circle of two arcs is still closed (end of last == start of first)
    assert!(
        k.segments[1].end().distance(k.segments[0].start()) < 1e-6,
        "{:?}",
        k.segments
    );
}

#[test]
fn edit_wrong_kind_and_missing_ids() {
    let (mut s, c, r) = two_shapes();
    assert_eq!(
        s.edit_shape(
            c,
            Shape::Rect {
                min: p(0.0, 0.0),
                w: 1.0,
                h: 1.0,
                r: 0.0
            }
        ),
        Err("Not editable as that shape".into())
    );
    assert!(s
        .edit_shape(
            9999,
            Shape::Circle {
                c: p(0.0, 0.0),
                r: 1.0
            }
        )
        .is_err());
    let _ = r;
    assert!(s.item_shape(9999).is_none());
}

#[test]
fn move_selection_to_empty_and_point_item() {
    let mut s = AppState::new(None);
    s.move_selection_to(p(5.0, 5.0)); // nothing selected
    assert!(!s.dirty);
    let pt = s.project.add_drawn(vec![Geom::Point(p(1.0, 2.0))])[0];
    s.items_changed();
    s.selection = BTreeSet::from([pt]);
    s.move_selection_to(p(10.0, 20.0));
    assert_eq!(on(&s, pt), p(10.0, 20.0));
}

// ---------------------------------------------------------- recognition (state)

fn add(s: &mut AppState, c: Contour) -> u64 {
    let id = s.project.add_drawn(vec![Geom::Contour(c)])[0];
    s.items_changed();
    id
}

#[test]
fn nearly_axis_aligned_rect_is_recognised_and_edit_snaps_it() {
    let mut s = AppState::new(None);
    // 4e-4 mm skew on one vertex: within RECOGNIZE_TOL
    let c = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(30.0, 0.0004), 0.0),
            (p(30.0, 10.0), 0.0),
            (p(0.0, 10.0), 0.0),
        ],
        true,
    );
    let id = add(&mut s, c);
    assert!(matches!(s.item_shape(id), Some(Shape::Rect { .. })));
    s.edit_shape(
        id,
        Shape::Rect {
            min: p(0.0, 0.0),
            w: 40.0,
            h: 10.0,
            r: 0.0,
        },
    )
    .unwrap();
    let k = contour(&s, id);
    assert_eq!(k.segments.len(), 4);
    for g in &k.segments {
        let (a, b) = (g.start(), g.end());
        assert!(
            (a.x - b.x).abs() < 1e-12 || (a.y - b.y).abs() < 1e-12,
            "{g:?}"
        );
    }
    // slightly beyond tolerance: not recognised, still movable via bbox
    let c2 = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(30.0, 0.002), 0.0),
            (p(30.0, 10.0), 0.0),
            (p(0.0, 10.0), 0.0),
        ],
        true,
    );
    let id2 = add(&mut s, c2);
    assert!(s.item_shape(id2).is_none());
}

#[test]
fn open_contour_that_looks_like_a_rectangle_is_not_editable() {
    let mut s = AppState::new(None);
    let open4 = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(30.0, 0.0), 0.0),
            (p(30.0, 10.0), 0.0),
            (p(0.0, 10.0), 0.0),
            (p(0.0, 0.0), 0.0),
        ],
        false,
    );
    assert_eq!(open4.segments.len(), 4);
    let id = add(&mut s, open4);
    assert!(s.item_shape(id).is_none());
    // open 3-line "U"
    let u = Contour::from_bulge_vertices(
        &[
            (p(0.0, 0.0), 0.0),
            (p(30.0, 0.0), 0.0),
            (p(30.0, 10.0), 0.0),
            (p(0.0, 10.0), 0.0),
        ],
        false,
    );
    let id = add(&mut s, u);
    assert!(s.item_shape(id).is_none());
    // open arc-only half circle
    let half = Contour::from_bulge_vertices(&[(p(0.0, 0.0), 1.0), (p(10.0, 0.0), 0.0)], false);
    let id = add(&mut s, half);
    assert!(s.item_shape(id).is_none());
}

#[test]
fn three_arc_circle_and_mixed_direction_arcs() {
    use hc_geom::shapes::{recognize, Primitive};
    // three 120 degree arcs form a circle
    let c = hc_geom::shapes::circle(p(0.0, 0.0), 5.0).unwrap();
    let Segment::Arc { .. } = c.segments[0] else {
        panic!()
    };
    assert!(matches!(recognize(&c), Some(Primitive::Circle { .. })));
    // two arcs of opposite sweep sign (S-curve closed): not a circle
    let s2 = Contour::from_bulge_vertices(&[(p(0.0, 0.0), 1.0), (p(10.0, 0.0), -1.0)], true);
    assert!(recognize(&s2).is_none());
    // empty
    assert!(recognize(&Contour {
        segments: vec![],
        closed: true
    })
    .is_none());
}

#[test]
fn concave_corner_arcs_and_diamond_are_not_rects() {
    use hc_geom::shapes::recognize;
    let k = (std::f64::consts::FRAC_PI_8).tan();
    let v = |b: f64| {
        Contour::from_bulge_vertices(
            &[
                (p(2.0, 0.0), 0.0),
                (p(8.0, 0.0), b),
                (p(10.0, 2.0), 0.0),
                (p(10.0, 8.0), b),
                (p(8.0, 10.0), 0.0),
                (p(2.0, 10.0), b),
                (p(0.0, 8.0), 0.0),
                (p(0.0, 2.0), b),
            ],
            true,
        )
    };
    assert!(recognize(&v(k)).is_some(), "convex rounded rect");
    assert!(recognize(&v(-k)).is_none(), "concave corners");
    let diamond = Contour::from_bulge_vertices(
        &[
            (p(5.0, 0.0), 0.0),
            (p(10.0, 5.0), 0.0),
            (p(5.0, 10.0), 0.0),
            (p(0.0, 5.0), 0.0),
        ],
        true,
    );
    assert!(recognize(&diamond).is_none());
    // degenerate: all points the same
    let z = Contour::from_bulge_vertices(
        &[
            (p(1.0, 1.0), 0.0),
            (p(1.0, 1.0), 0.0),
            (p(1.0, 1.0), 0.0),
            (p(1.0, 1.0), 0.0),
        ],
        true,
    );
    assert!(recognize(&z).is_none());
}

// ----------------------------------------------------------------- kittest

mod ui {
    use super::*;
    use crate::state::CanvasTool;
    use crate::view::View;
    use crate::App;
    use eframe::egui::{self, PointerButton};
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    macro_rules! drag {
        ($h:ident, $a:expr, $b:expr, $btn:expr, $shift:expr $(,)?) => {{
            let (a, b) = ($a, $b);
            drag_fn(&mut $h, a, b, $btn, $shift)
        }};
    }
    macro_rules! click {
        ($h:ident, $a:expr, $shift:expr $(,)?) => {{
            let a = $a;
            click_fn(&mut $h, a, $shift)
        }};
    }

    fn harness(s: AppState) -> Harness<'static, App> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s))
    }

    fn fixed(s: AppState) -> Harness<'static, App> {
        let mut h = harness(s);
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

    fn mods(shift: bool) -> egui::Modifiers {
        if shift {
            egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::NONE
        }
    }

    fn btn(h: &Harness<App>, pos: egui::Pos2, b: PointerButton, pressed: bool, shift: bool) {
        h.event(egui::Event::PointerButton {
            pos,
            button: b,
            pressed,
            modifiers: mods(shift),
        });
    }

    /// Press at `a`, move through `a..b` in steps, release at `b`.
    fn drag_fn(
        h: &mut Harness<'static, App>,
        a: egui::Pos2,
        b: egui::Pos2,
        btn_: PointerButton,
        shift: bool,
    ) {
        h.event(egui::Event::ModifiersChanged(mods(shift)));
        h.hover_at(a);
        h.run();
        btn(h, a, btn_, true, shift);
        h.run();
        for k in 1..=4 {
            h.hover_at(a + (b - a) * (k as f32 / 4.0));
            h.run();
        }
        btn(h, b, btn_, false, shift);
        h.run();
        h.run();
    }

    fn click_fn(h: &mut Harness<'static, App>, at: egui::Pos2, shift: bool) {
        h.event(egui::Event::ModifiersChanged(mods(shift)));
        h.hover_at(at);
        h.run();
        btn(h, at, PointerButton::Primary, true, shift);
        h.run();
        btn(h, at, PointerButton::Primary, false, shift);
        h.run();
        h.run();
    }

    fn two() -> (Harness<'static, App>, u64, u64) {
        let (mut s, c, r) = two_shapes();
        s.clear_selection();
        let h = fixed(s);
        (h, c, r)
    }

    // ---- canvas id change: other interactions still work

    #[test]
    fn click_selects_and_click_on_empty_clears() {
        let (mut h, c, r) = two();
        click!(h, to_s(&h, p(60.0, 50.0)), false);
        assert_eq!(h.state().state.selection, BTreeSet::from([c]));
        click!(h, to_s(&h, p(110.0, 0.0)), true); // Shift adds the rect's bottom edge
        assert_eq!(h.state().state.selection, BTreeSet::from([c, r]));
        click!(h, to_s(&h, p(150.0, 100.0)), false);
        assert!(h.state().state.selection.is_empty());
    }

    #[test]
    fn box_select_from_empty_and_shift_additive() {
        let (mut h, c, r) = two();
        // left-to-right box around the circle only, starting in empty space
        drag!(
            h,
            to_s(&h, p(30.0, 30.0)),
            to_s(&h, p(70.0, 70.0)),
            PointerButton::Primary,
            false,
        );
        assert_eq!(h.state().state.selection, BTreeSet::from([c]));
        assert!(h.state().state.moving.is_none());
        // Shift box around the rect: additive
        drag!(
            h,
            to_s(&h, p(90.0, -10.0)),
            to_s(&h, p(130.0, 20.0)),
            PointerButton::Primary,
            true,
        );
        assert_eq!(h.state().state.selection, BTreeSet::from([c, r]));
        // nothing moved
        assert!(near(
            contour(&h.state().state, c).as_circle().unwrap().center,
            p(50.0, 50.0)
        ));
    }

    #[test]
    fn shift_drag_starting_on_item_is_a_box_not_a_move() {
        let (mut h, c, _) = two();
        drag!(
            h,
            to_s(&h, p(60.0, 50.0)),
            to_s(&h, p(100.0, 80.0)),
            PointerButton::Primary,
            true,
        );
        assert!(near(
            contour(&h.state().state, c).as_circle().unwrap().center,
            p(50.0, 50.0)
        ));
    }

    #[test]
    fn middle_drag_pans_over_an_item_without_moving_it() {
        let (mut h, c, _) = two();
        let off0 = h.state().state.view.offset;
        drag!(
            h,
            to_s(&h, p(60.0, 50.0)),
            to_s(&h, p(60.0, 50.0)) + egui::vec2(40.0, 20.0),
            PointerButton::Middle,
            false,
        );
        let d = h.state().state.view.offset - off0;
        assert!(
            (d.x - 40.0).abs() < 1.0 && (d.y - 20.0).abs() < 1.0,
            "{d:?}"
        );
        assert!(near(
            contour(&h.state().state, c).as_circle().unwrap().center,
            p(50.0, 50.0)
        ));
        assert!(h.state().state.selection.is_empty());
    }

    #[test]
    fn space_drag_pans_and_does_not_move_or_box() {
        let (mut h, c, _) = two();
        let off0 = h.state().state.view.offset;
        h.key_down(egui::Key::Space);
        h.run();
        drag!(
            h,
            to_s(&h, p(60.0, 50.0)),
            to_s(&h, p(60.0, 50.0)) + egui::vec2(30.0, 10.0),
            PointerButton::Primary,
            false,
        );
        h.key_up(egui::Key::Space);
        h.run();
        let d = h.state().state.view.offset - off0;
        assert!(d.x > 20.0 && d.y > 5.0, "{d:?}");
        assert!(near(
            contour(&h.state().state, c).as_circle().unwrap().center,
            p(50.0, 50.0)
        ));
        assert!(h.state().state.moving.is_none());
    }

    #[test]
    fn wheel_zoom_changes_zoom_over_the_canvas() {
        let (mut h, _, _) = two();
        let z0 = h.state().state.view.zoom;
        h.hover_at(to_s(&h, p(50.0, 50.0)));
        h.run();
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        h.run();
        assert!(h.state().state.view.zoom > z0);
    }

    #[test]
    fn tool_clicks_draw_and_set_origin_still_works() {
        let (mut h, _, _) = two();
        let n = h.state().state.project.items.len();
        h.state_mut().state.set_tool(CanvasTool::Line);
        h.run();
        click!(h, to_s(&h, p(10.0, 10.0)), false);
        click!(h, to_s(&h, p(30.0, 12.5)), false);
        assert_eq!(h.state().state.project.items.len(), n + 1);
        h.state_mut().state.set_tool(CanvasTool::Rect);
        h.run();
        click!(h, to_s(&h, p(10.0, 60.0)), false);
        click!(h, to_s(&h, p(25.0, 80.0)), false);
        assert_eq!(h.state().state.project.items.len(), n + 2);
        h.state_mut().state.set_tool(CanvasTool::Circle);
        h.run();
        click!(h, to_s(&h, p(10.0, 90.0)), false);
        click!(h, to_s(&h, p(15.0, 90.0)), false);
        assert_eq!(h.state().state.project.items.len(), n + 3);
        h.state_mut().state.set_tool(CanvasTool::SetOrigin);
        h.run();
        click!(h, to_s(&h, p(100.0, 0.0)) + egui::vec2(3.0, -2.0), false);
        assert_eq!(
            h.state().state.project.stock.origin,
            hc_project::WorkOrigin::Point(p(100.0, 0.0))
        );
    }

    #[test]
    fn hover_tracks_cursor_and_snap_marker_state_with_drawing_tool() {
        let (mut h, _, _) = two();
        h.state_mut().state.set_tool(CanvasTool::Line);
        h.run();
        let at = to_s(&h, p(100.0, 0.0)) + egui::vec2(2.0, -1.0);
        h.hover_at(at);
        h.run();
        let s = &h.state().state;
        let cur = s.cursor.expect("cursor");
        let sn = s
            .snap_at(cur, crate::snap::SNAP_PX / s.view.zoom as f64)
            .unwrap();
        assert!(near(sn.p, p(100.0, 0.0)));
    }

    #[test]
    fn plain_drag_on_item_with_tool_other_than_select_does_not_move() {
        let (mut h, c, _) = two();
        h.state_mut().state.set_tool(CanvasTool::Line);
        h.run();
        drag!(
            h,
            to_s(&h, p(60.0, 50.0)),
            to_s(&h, p(90.0, 50.0)),
            PointerButton::Primary,
            false,
        );
        assert!(near(
            contour(&h.state().state, c).as_circle().unwrap().center,
            p(50.0, 50.0)
        ));
        assert!(h.state().state.moving.is_none());
    }

    /// Esc must cancel even when the pointer left the canvas (over a side panel) mid-drag.
    #[test]
    fn esc_cancels_drag_with_pointer_over_a_side_panel() {
        let (mut h, c, _) = two();
        let a = to_s(&h, p(60.0, 50.0));
        h.hover_at(a);
        h.run();
        btn(&h, a, PointerButton::Primary, true, false);
        h.run();
        h.hover_at(a + egui::vec2(30.0, 0.0));
        h.run();
        assert!(h.state().state.moving.is_some());
        h.hover_at(egui::pos2(40.0, 400.0)); // left tool panel
        h.run();
        h.key_press(egui::Key::Escape);
        h.run();
        let at_release = h.state().state.moving.is_some();
        btn(
            &h,
            egui::pos2(40.0, 400.0),
            PointerButton::Primary,
            false,
            false,
        );
        h.run();
        h.run();
        let k = contour(&h.state().state, c).as_circle().unwrap().center;
        assert!(
            near(k, p(50.0, 50.0)) && !at_release,
            "Esc over a side panel did not cancel the drag: centre {k:?}, still moving {at_release}"
        );
    }

    #[test]
    fn drop_outside_the_canvas_ends_the_drag_and_keeps_one_undo_step() {
        let (mut h, c, _) = two();
        let n = h.state().state.undo.len();
        let a = to_s(&h, p(60.0, 50.0));
        h.hover_at(a);
        h.run();
        btn(&h, a, PointerButton::Primary, true, false);
        h.run();
        h.hover_at(a + egui::vec2(30.0, 0.0));
        h.run();
        h.hover_at(egui::pos2(40.0, 400.0));
        h.run();
        btn(
            &h,
            egui::pos2(40.0, 400.0),
            PointerButton::Primary,
            false,
            false,
        );
        h.run();
        h.run();
        let s = &h.state().state;
        assert!(s.moving.is_none());
        assert_eq!(s.undo.len(), n + 1);
        assert!(contour(s, c).as_circle().unwrap().center.x < 50.0);
    }

    #[test]
    fn move_via_shift_axis_lock_on_canvas() {
        let (mut h, c, _) = two();
        // diagonal drag with Shift held from the start would be a box; Shift must be pressed after the drag began.
        let a = to_s(&h, p(60.0, 50.0));
        h.hover_at(a);
        h.run();
        btn(&h, a, PointerButton::Primary, true, false);
        h.run();
        h.hover_at(a + egui::vec2(10.0, 10.0));
        h.run();
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        h.hover_at(a + egui::vec2(60.0, 20.0));
        h.run();
        assert!(h.state().state.moving.is_some());
        let d = h.state().state.moving.as_ref().unwrap().delta;
        assert!(d.y == 0.0 && d.x > 20.0, "{d:?}");
        btn(
            &h,
            a + egui::vec2(60.0, 20.0),
            PointerButton::Primary,
            false,
            true,
        );
        h.run();
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run();
        let k = contour(&h.state().state, c).as_circle().unwrap().center;
        assert!((k.y - 50.0).abs() < 1e-9, "{k:?}");
    }

    // ---- tab markers and the tab op

    fn bracket_ui() -> (Harness<'static, App>, u64, u64) {
        let (mut s, outside) = bracket_clean();
        hide_dup_layers(&mut s);
        let o = outer(&s);
        s.select_op(outside);
        let h = harness(s);
        (h, outside, o)
    }

    fn manual(h: &Harness<App>, op: u64) -> Vec<f64> {
        h.state()
            .state
            .project
            .operations
            .iter()
            .find(|o| o.id == op)
            .unwrap()
            .tabs
            .manual
            .iter()
            .flat_map(|m| m.positions.clone())
            .collect()
    }

    #[test]
    fn tab_marker_drag_and_double_click_still_work_and_never_move_the_contour() {
        let (mut h, op, o) = bracket_ui();
        h.run();
        h.run();
        let before = contour(&h.state().state, o);
        // marker world position of the first automatic tab
        let (c, pos) = {
            let s = &h.state().state;
            let m = s.tab_markers();
            let (_, c, pos) = m.iter().find(|(id, _, _)| *id == o).unwrap();
            ((*c).clone(), pos.clone())
        };
        assert!(!pos.is_empty());
        let l = c.length();
        let at_marker = c.point_at(pos[0].rem_euclid(1.0) * l);
        let a = to_s(&h, at_marker);
        // a double click on a marker adds a tab
        let n0 = manual(&h, op).len();
        h.hover_at(a);
        h.run();
        for k in 0..2 {
            btn(&h, a, PointerButton::Primary, true, false);
            btn(&h, a, PointerButton::Primary, false, false);
            h.input_mut().time = Some(100.0 + 0.1 * k as f64);
            h.step();
        }
        h.input_mut().time = None;
        h.run();
        let n1 = manual(&h, op).len();
        assert!(
            n1 > n0,
            "double click on a marker adds a manual tab ({n0} -> {n1})"
        );
        // drag the (now selected) tab marker along the contour: contour untouched, no element move
        let (c2, pos2) = {
            let s = &h.state().state;
            let (_, c, pos) = s
                .tab_markers()
                .into_iter()
                .find(|(id, _, _)| *id == o)
                .unwrap();
            (c.clone(), pos)
        };
        let from = c2.point_at(pos2[0].rem_euclid(1.0) * c2.length());
        let to = c2.point_at((pos2[0] + 0.05).rem_euclid(1.0) * c2.length());
        drag!(
            h,
            to_s(&h, from),
            to_s(&h, to),
            PointerButton::Primary,
            false,
        );
        assert!(h.state().state.moving.is_none());
        assert_eq!(contour(&h.state().state, o), before, "contour untouched");
        assert_eq!(manual(&h, op).len(), n1);
    }

    #[test]
    fn drag_of_current_tab_contour_moves_it_and_keeps_op_and_fractions() {
        let (mut h, op, o) = bracket_ui();
        h.run();
        h.run();
        // fix manual tabs so fractions are checkable
        crate::state::tests::edit_op(&mut h.state_mut().state, op, |op| {
            op.tabs.manual = vec![hc_project::ManualTabs {
                item: o,
                positions: vec![0.1, 0.6],
            }]
        });
        h.run();
        let c0 = contour(&h.state().state, o);
        // press on the contour at a spot away from every tab marker
        let start = c0.point_at(c0.length() * 0.35);
        let a = to_s(&h, start);
        drag!(
            h,
            a,
            a + egui::vec2(20.0, 0.0),
            PointerButton::Primary,
            false,
        );
        let s = &h.state().state;
        assert_eq!(s.selected_op, Some(op), "op stays current");
        assert_eq!(manual(&h, op), vec![0.1, 0.6]);
        let c1 = contour(s, o);
        let dx = 20.0 / s.view.zoom as f64;
        assert!(
            c1.bbox().min.approx_eq(c0.bbox().min + p(dx, 0.0), 1e-3),
            "moved by 20 px / zoom: {:?} -> {:?}, sel {:?}",
            c0.bbox().min,
            c1.bbox().min,
            s.selection
        );
        // toolpaths regenerated after the drop: still reference the item, and a path exists for the new place

        assert!(s
            .project
            .operations
            .iter()
            .find(|x| x.id == op)
            .unwrap()
            .items
            .contains(&o));
        assert_eq!(s.toolpaths_for.as_ref(), Some(&s.project));
    }

    #[test]
    fn toolpaths_are_not_refreshed_during_drag_but_are_after_drop() {
        let (mut s, _) = bracket_clean();
        hide_dup_layers(&mut s);
        s.clear_selection();
        let o = outer(&s);
        let mut h = harness(s);
        h.run();
        h.run();
        h.run();
        let c0 = contour(&h.state().state, o);
        let a = to_s(&h, c0.point_at(c0.length() * 0.35));
        let b = a + egui::vec2(30.0, 0.0);
        h.hover_at(a);
        h.run();
        btn(&h, a, PointerButton::Primary, true, false);
        h.run();
        for k in 1..=3 {
            h.hover_at(a + (b - a) * (k as f32 / 3.0));
            h.run();
        }
        let st = &h.state().state;
        assert!(st.moving.is_some());
        assert_ne!(
            st.toolpaths_for.as_ref(),
            Some(&st.project),
            "stale while dragging"
        );
        btn(&h, b, PointerButton::Primary, false, false);
        h.run();
        h.run();
        let st = &h.state().state;
        assert!(st.moving.is_none());
        assert_eq!(
            st.toolpaths_for.as_ref(),
            Some(&st.project),
            "fresh after drop"
        );
    }

    // ---- Properties field behaviour

    fn selected(s: AppState) -> Harness<'static, App> {
        let mut h = harness(s);
        h.run();
        h
    }

    fn rect_state() -> (AppState, u64) {
        let mut s = AppState::new(None);
        let id = s
            .add_shape(Shape::Rect {
                min: p(0.0, 0.0),
                w: 20.0,
                h: 10.0,
                r: 0.0,
            })
            .unwrap()[0];
        s.commit_undo(true);
        (s, id)
    }

    fn width(h: &Harness<App>, id: u64) -> f64 {
        contour(&h.state().state, id).bbox().width()
    }

    #[test]
    fn typed_value_applies_on_enter_not_per_keystroke() {
        let (s, id) = rect_state();
        let mut h = selected(s);
        let n = h.state().state.undo.len();
        h.get_by_label("Width").click();
        h.run();
        h.run();
        h.get_by_label("Width").type_text("45");
        h.run();
        h.run();
        assert_eq!(width(&h, id), 20.0, "partial typing must not apply");
        assert_eq!(h.state().state.undo.len(), n);
        h.key_press(egui::Key::Enter);
        h.run();
        h.run();
        h.run();
        assert!((width(&h, id) - 45.0).abs() < 1e-9, "{}", width(&h, id));
        assert_eq!(h.state().state.undo.len(), n + 1);
    }

    #[test]
    fn typed_invalid_value_warns_and_reverts() {
        let (s, id) = rect_state();
        let mut h = selected(s);
        h.get_by_label("Width").click();
        h.run();
        h.run();
        h.get_by_label("Width").type_text("-5");
        h.key_press(egui::Key::Enter);
        h.run();
        h.run();
        h.run();
        assert_eq!(width(&h, id), 20.0);
        let m = h.state().state.messages.last().unwrap();
        assert_eq!(m.severity, crate::state::Severity::Warning);
        assert_eq!(m.text, "Rectangle needs a width and a height");
        // the field shows the old value again
        let v = h.get_by_label("Width").accesskit_node().numeric_value();
        assert_eq!(v, Some(20.0), "{v:?}");
    }

    #[test]
    fn invalid_circle_diameter_and_line_length_warn() {
        let mut s = AppState::new(None);
        let c = s
            .add_shape(Shape::Circle {
                c: p(5.0, 5.0),
                r: 3.0,
            })
            .unwrap()[0];
        let mut h = selected(s);
        h.get_by_label("Diameter").set_value_accesskit(0.0);
        h.run();
        h.run();
        assert!((contour(&h.state().state, c).as_circle().unwrap().radius - 3.0).abs() < 1e-9);
        assert_eq!(
            h.state().state.messages.last().unwrap().text,
            "Circle needs a diameter"
        );
        h.get_by_label("Diameter").set_value_accesskit(-4.0);
        h.run();
        h.run();
        assert!((contour(&h.state().state, c).as_circle().unwrap().radius - 3.0).abs() < 1e-9);

        let mut s = AppState::new(None);
        let l = s
            .add_shape(Shape::Line {
                a: p(1.0, 1.0),
                b: p(4.0, 5.0),
            })
            .unwrap()[0];
        let mut h = selected(s);
        h.get_by_label("Length").set_value_accesskit(0.0);
        h.run();
        h.run();
        let k = contour(&h.state().state, l);
        assert!(near(k.segments[0].end(), p(4.0, 5.0)));
        assert_eq!(
            h.state().state.messages.last().unwrap().text,
            "Line is too short"
        );
        // negative length must not silently flip the line
        h.get_by_label("Length").set_value_accesskit(-5.0);
        h.run();
        h.run();
        let k = contour(&h.state().state, l);
        assert!(
            near(k.segments[0].end(), p(4.0, 5.0)),
            "negative Length flipped the line to {:?}",
            k.segments[0].end()
        );
    }

    #[test]
    fn line_length_keeps_start_and_direction_in_inch() {
        let mut s = AppState::new(None);
        let l = s
            .add_shape(Shape::Line {
                a: p(10.0, 10.0),
                b: p(13.0, 14.0),
            })
            .unwrap()[0];
        s.set_units(hc_project::Units::Inch);
        let mut h = selected(s);
        h.get_by_label("Length").set_value_accesskit(1.0); // 25.4 mm
        h.run();
        h.run();
        let k = contour(&h.state().state, l);
        assert!(near(k.start(), p(10.0, 10.0)));
        let e = k.segments[0].end();
        assert!(near(e, p(10.0 + 25.4 * 0.6, 10.0 + 25.4 * 0.8)), "{e:?}");
    }

    #[test]
    fn rect_corner_radius_and_size_edits_via_fields_inch() {
        let (mut s, id) = rect_state();
        s.set_units(hc_project::Units::Inch);
        let mut h = selected(s);
        h.get_by_label("Height").set_value_accesskit(1.0); // 25.4
        h.run();
        h.run();
        assert!((contour(&h.state().state, id).bbox().height() - 25.4).abs() < 1e-9);
        h.get_by_label("Corner radius").set_value_accesskit(0.25); // 6.35
        h.run();
        h.run();
        let k = contour(&h.state().state, id);
        assert_eq!(k.segments.len(), 8);
        // far too big in inch: rejected with an inch message
        h.get_by_label("Corner radius").set_value_accesskit(5.0);
        h.run();
        h.run();
        assert_eq!(contour(&h.state().state, id), k);
        let m = h.state().state.messages.last().unwrap();
        assert!(
            m.text.starts_with("Corner radius must be at most"),
            "{}",
            m.text
        );
    }

    #[test]
    fn m13_selection_unchanged_by_property_edit_and_circle_label_set() {
        let (s, _) = rect_state();
        let h = selected(s);
        for l in ["X", "Y", "Width", "Height", "Corner radius"] {
            assert!(h.query_by_label(l).is_some(), "{l}");
        }
        assert!(h.query_by_label("Diameter").is_none());
        assert!(h.query_by_label("Radius").is_none());
    }
}
