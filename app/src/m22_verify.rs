//! M22 acceptance tests: Trim and Extend tools.

use crate::state::tests::{op_on, tool};
use crate::state::Severity;
use crate::state::{AppState, CanvasTool, Shape};
use hc_geom::{Contour, Point as P, Segment};
use hc_project::{Geom, Item, ManualTabs, Source};

const TOL: f64 = 0.5;

fn line(s: &mut AppState, a: (f64, f64), b: (f64, f64)) -> u64 {
    s.add_shape(Shape::Line {
        a: P::new(a.0, a.1),
        b: P::new(b.0, b.1),
    })
    .unwrap()[0]
}

fn circle(s: &mut AppState, c: (f64, f64), r: f64) -> u64 {
    s.add_shape(Shape::Circle {
        c: P::new(c.0, c.1),
        r,
    })
    .unwrap()[0]
}

fn contour(s: &AppState, id: u64) -> Contour {
    match &s.item(id).unwrap().geom {
        Geom::Contour(c) => c.clone(),
        Geom::Point(_) => panic!(),
    }
}

fn ends(s: &AppState, id: u64) -> ((f64, f64), (f64, f64)) {
    let c = contour(s, id);
    let (a, b) = (c.start(), c.end());
    ((a.x, a.y), (b.x, b.y))
}

fn near(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> bool {
    let d = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() + (p.1 - q.1).abs() < 1e-9;
    d(a.0, b.0) && d(a.1, b.1)
}

fn put(s: &mut AppState, c: Contour) -> u64 {
    let id = s.project.next_id;
    s.project.next_id += 1;
    s.project.items.push(Item {
        id,
        layer: "Drawing".into(),
        source: Source::Drawn,
        geom: Geom::Contour(c),
    });
    s.items_changed();
    id
}

fn open_poly(pts: &[(f64, f64)]) -> Contour {
    Contour {
        segments: pts
            .windows(2)
            .map(|w| Segment::Line {
                start: P::new(w[0].0, w[0].1),
                end: P::new(w[1].0, w[1].1),
            })
            .collect(),
        closed: false,
    }
}

fn start(s: &mut AppState, t: CanvasTool, limits: &[u64]) {
    s.selection = limits.iter().copied().collect();
    s.set_tool(t);
}

/// a = (0,0)-(10,0), limits x = 3 and x = 7 (selected), Trim active, engrave op on a.
fn split_setup() -> (AppState, u64, u64, [u64; 2]) {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let op = op_on(&mut s, vec![a], crate::state::NewKind::Engrave, 1);
    let l1 = line(&mut s, (3.0, -5.0), (3.0, 5.0));
    let l2 = line(&mut s, (7.0, -5.0), (7.0, 5.0));
    start(&mut s, CanvasTool::Trim, &[l1, l2]);
    s.dirty = false;
    (s, a, op, [l1, l2])
}

#[test]
fn review_undone_split_id_is_not_a_limit() {
    let mut s = AppState::new(None);
    let h = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let l1 = line(&mut s, (3.0, -5.0), (3.0, 5.0));
    let l2 = line(&mut s, (7.0, -5.0), (7.0, 5.0));
    let c = line(&mut s, (0.0, 2.0), (10.0, 2.0));
    start(&mut s, CanvasTool::Trim, &[h, l1, l2]);
    s.commit_undo(true);
    s.trim_click(P::new(5.0, 0.0), TOL); // splits the limit h: its new part joins the limits
    let n = s.project.items.iter().map(|i| i.id).max().unwrap();
    assert!(s.trim_limits.contains(&n));
    s.commit_undo(true);
    s.undo();
    s.trim_click(P::new(5.0, 2.0), TOL); // splits c (not a limit); the new part reuses id n
    assert!(s.project.items.iter().any(|i| i.id == n));
    assert!(!s.trim_limits.contains(&n));
    assert!(near(ends(&s, c), ((0.0, 2.0), (3.0, 2.0))));
}

#[test]
fn review_trim_clears_selected_tab() {
    let (mut s, a, _, _) = split_setup();
    s.selected_tab = Some((a, 0));
    s.trim_click(P::new(5.0, 0.0), TOL);
    assert_eq!(s.selected_tab, None);
}

#[test]
fn s1_no_limits() {
    for (t, text) in [
        (CanvasTool::Trim, "Trim: select the limiting objects first"),
        (
            CanvasTool::Extend,
            "Extend: select the limiting objects first",
        ),
    ] {
        let mut s = AppState::new(None);
        line(&mut s, (0.0, 0.0), (10.0, 0.0));
        start(&mut s, t, &[]);
        s.dirty = false;
        let (before, n) = (s.project.clone(), s.messages.len());
        assert_eq!(s.hint(), Some(text));
        assert!(s.trim_hover(P::new(8.0, 0.0), TOL).is_none());
        s.trim_click(P::new(8.0, 0.0), TOL);
        assert_eq!(s.project, before);
        assert_eq!(s.messages.len(), n);
        assert!(!s.dirty);
    }
}

#[test]
fn s2_trim_and_undo() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut s, (4.0, -5.0), (4.0, 5.0));
    start(&mut s, CanvasTool::Trim, &[b]);
    s.commit_undo(true);
    let n = s.undo.len();
    s.trim_click(P::new(8.0, 0.1), TOL);
    assert!(near(ends(&s, a), ((0.0, 0.0), (4.0, 0.0))));
    assert!(s.dirty);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
    s.undo();
    assert!(near(ends(&s, a), ((0.0, 0.0), (10.0, 0.0))));
}

#[test]
fn s3_split_and_operation() {
    let (mut s, a, op, _) = split_setup();
    s.trim_click(P::new(5.0, 0.0), TOL);
    let n = s.project.items[1].id;
    assert!(near(ends(&s, a), ((0.0, 0.0), (3.0, 0.0))));
    assert!(near(ends(&s, n), ((7.0, 0.0), (10.0, 0.0))));
    assert_eq!(s.project.items[0].id, a);
    assert_eq!(s.project.items[1].layer, "Drawing");
    assert_eq!(s.project.items[1].source, Source::Drawn);
    assert_eq!(
        s.project
            .operations
            .iter()
            .find(|o| o.id == op)
            .unwrap()
            .items,
        [a, n]
    );
    s.refresh_toolpaths();
    assert!(s.toolpath_result(op).unwrap().is_ok());
}

#[test]
fn s4_closed_opened() {
    let mut s = AppState::new(None);
    s.library.add(tool(1)).unwrap();
    let r = s
        .add_shape(Shape::Rect {
            min: P::new(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let op = op_on(&mut s, vec![r], crate::state::NewKind::Contour, 1);
    let l = line(&mut s, (10.0, -5.0), (10.0, 15.0));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.trim_click(P::new(20.0, 10.0), TOL);
    let c = contour(&s, r);
    assert!(!c.closed && c.segments.len() == 4);
    assert!((c.length() - 60.0).abs() < 1e-9);
    s.refresh_toolpaths();
    assert!(s.toolpath_result(op).unwrap().is_err());
}

#[test]
fn s5_circle_to_arc() {
    let mut s = AppState::new(None);
    let c = circle(&mut s, (0.0, 0.0), 5.0);
    let l = line(&mut s, (0.0, -10.0), (0.0, 10.0));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.trim_click(P::new(5.1, 0.0), TOL);
    let k = contour(&s, c);
    assert!(!k.closed && k.segments.len() == 1);
    assert!(matches!(k.segments[0], Segment::Arc { .. }));
    assert!(k.start().distance(P::new(0.0, 5.0)) < 1e-9);
    assert!(k.end().distance(P::new(0.0, -5.0)) < 1e-9);
    assert!(s.item_shape(c).is_none());
}

#[test]
fn s6_extend() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let c = circle(&mut s, (20.0, 0.0), 5.0);
    start(&mut s, CanvasTool::Extend, &[c]);
    s.commit_undo(true);
    let n = s.undo.len();
    s.trim_click(P::new(9.0, 0.1), TOL);
    assert_eq!(
        s.item_shape(a),
        Some(Shape::Line {
            a: P::new(0.0, 0.0),
            b: P::new(15.0, 0.0)
        })
    );
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
    s.trim_click(P::new(1.0, 0.1), TOL);
    let m = s.messages.last().unwrap();
    assert_eq!(m.severity, Severity::Warning);
    assert_eq!(m.text, "No limit in that direction");
    assert!(near(ends(&s, a), ((0.0, 0.0), (15.0, 0.0))));
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n + 1);
}

#[test]
fn s7_nothing_to_trim() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let l = line(&mut s, (20.0, -5.0), (20.0, 5.0));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.dirty = false;
    s.trim_click(P::new(5.0, 0.1), TOL);
    let m = s.messages.last().unwrap();
    assert_eq!(m.severity, Severity::Warning);
    assert_eq!(m.text, "Nothing to trim here");
    assert!(near(ends(&s, a), ((0.0, 0.0), (10.0, 0.0))));
    assert!(!s.dirty);
}

#[test]
fn s8_frozen_limits() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut s, (4.0, -5.0), (4.0, 5.0));
    start(&mut s, CanvasTool::Trim, &[b]);
    s.clear_selection();
    s.trim_click(P::new(8.0, 0.1), TOL);
    assert!(near(ends(&s, a), ((0.0, 0.0), (4.0, 0.0))));

    let (mut s, a, _, [l1, l2]) = split_setup();
    start(&mut s, CanvasTool::Trim, &[a, l1, l2]);
    s.trim_click(P::new(5.0, 0.0), TOL);
    let n = s.project.items[1].id;
    assert!(s.trim_limits.contains(&n) && s.selection.contains(&n));
}

#[test]
fn s9_tabs() {
    let (mut s, a, op, _) = split_setup();
    s.project.operations[0].tabs.manual = vec![ManualTabs {
        item: a,
        positions: vec![0.1, 0.5, 0.9],
    }];
    s.trim_click(P::new(5.0, 0.0), TOL);
    let n = s.project.items[1].id;
    let m = &s
        .project
        .operations
        .iter()
        .find(|o| o.id == op)
        .unwrap()
        .tabs
        .manual;
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].item, a);
    assert!((m[0].positions[0] - 1.0 / 3.0).abs() < 1e-9 && m[0].positions.len() == 1);
    assert_eq!(m[1].item, n);
    assert!((m[1].positions[0] - 2.0 / 3.0).abs() < 1e-9 && m[1].positions.len() == 1);

    let (mut s, a, _, _) = split_setup();
    s.project.operations[0].tabs.manual = vec![ManualTabs {
        item: a,
        positions: vec![0.5],
    }];
    s.trim_click(P::new(5.0, 0.0), TOL);
    assert!(s.project.operations[0].tabs.manual.is_empty());
}

#[test]
fn s10_picks() {
    let mut s = AppState::new(None);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let l = line(&mut s, (4.0, -5.0), (4.0, 5.0));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.set_layer_visible("Drawing", false);
    assert!(s.trim_hover(P::new(8.0, 0.0), TOL).is_none());

    let mut s = AppState::new(None);
    let r = s
        .add_shape(Shape::Rect {
            min: P::new(0.0, 0.0),
            w: 30.0,
            h: 10.0,
            r: 0.0,
        })
        .unwrap()[0];
    let p = put(
        &mut s,
        open_poly(&[(0.0, 20.0), (10.0, 20.0), (10.0, 30.0)]),
    );
    let l = line(&mut s, (-5.0, 40.0), (25.0, 40.0));
    start(&mut s, CanvasTool::Extend, &[l]);
    s.dirty = false;
    let n = s.messages.len();
    assert!(s.trim_hover(P::new(15.0, 0.0), TOL).is_none());
    s.trim_click(P::new(15.0, 0.0), TOL);
    assert_eq!(s.messages.len(), n);
    assert!(contour(&s, r).closed);
    assert_eq!(s.trim_hover(P::new(1.0, 20.1), TOL).unwrap().seg, 0);
    let h = s.trim_hover(P::new(10.1, 29.0), TOL).unwrap();
    assert_eq!((h.item, h.seg), (p, 1));
}

#[test]
fn s11_escape_and_set_tool() {
    let mut s = AppState::new(None);
    let l = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.escape();
    assert_eq!(s.tool, CanvasTool::Select);
    assert!(s.selection.contains(&l));
    start(&mut s, CanvasTool::Trim, &[l]);
    s.set_tool(CanvasTool::Line);
    assert!(s.trim_limits.is_empty());
    assert_eq!(
        CanvasTool::Trim.hint(false),
        Some("Trim: click the piece to cut away")
    );
}

#[test]
fn x1_split_undo_one_step_and_limits_survive() {
    let (mut s, a, op, lims) = split_setup();
    s.commit_undo(true);
    let n0 = s.undo.len();
    s.trim_click(P::new(5.0, 0.0), TOL);
    s.commit_undo(true);
    assert_eq!(s.undo.len(), n0 + 1);
    assert_eq!(s.project.items.len(), 4);
    s.undo();
    assert_eq!(s.project.items.len(), 3);
    assert!(near(ends(&s, a), ((0.0, 0.0), (10.0, 0.0))));
    let ids: Vec<u64> = s
        .project
        .operations
        .iter()
        .find(|o| o.id == op)
        .unwrap()
        .items
        .clone();
    assert_eq!(ids, [a]);
    // limits still frozen after undo: trimming again works
    assert_eq!(s.trim_limits, lims);
    s.trim_click(P::new(5.0, 0.0), TOL);
    assert_eq!(s.project.items.len(), 4);
}

#[test]
fn x2_hover_none_on_empty_canvas_and_miss() {
    let (s, _, _, _) = split_setup();
    assert!(s.trim_hover(P::new(50.0, 50.0), TOL).is_none());
    let h = s.trim_hover(P::new(5.0, 0.0), TOL).unwrap();
    assert!(h.piece.is_some());
    assert_eq!(h.parts.len(), 2);
    let mut s = s;
    let (before, n) = (s.project.clone(), s.messages.len());
    s.trim_click(P::new(50.0, 50.0), TOL);
    assert_eq!(s.project, before);
    assert_eq!(s.messages.len(), n);
}

#[test]
fn x3_repeated_extend_next_limit() {
    let mut s = AppState::new(None);
    let a = line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let l1 = line(&mut s, (10.0, -5.0), (10.0, 5.0));
    let l2 = line(&mut s, (20.0, -5.0), (20.0, 5.0));
    start(&mut s, CanvasTool::Extend, &[l1, l2]);
    s.trim_click(P::new(9.0, 0.1), TOL);
    assert!(near(ends(&s, a), ((0.0, 0.0), (20.0, 0.0))));
    s.trim_click(P::new(19.0, 0.1), TOL);
    assert!(near(ends(&s, a), ((0.0, 0.0), (20.0, 0.0))));
}
