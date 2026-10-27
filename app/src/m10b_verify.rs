//! M10b verifier tests (state): text through Pocket / Contour / Engrave, inch, transforms, undo, save/load, warnings.
//! Cut checks sample every feed move every 0.05 mm below Z0 and compare the tool edge with the glyph ink.

use crate::state::tests::{edit_op, scratch, tool};
use crate::state::{AppState, CanvasTool, NewKind, Severity, Shape};
use hc_geom::{Contour, Orientation, Point, Transform};
use hc_project::{Direction, Geom, OpKind, Side, Tool, Units};
use hc_text::Font;
use hc_toolpath::{Move, RuleId, ToolpathError};

const TOL: f64 = 2e-3;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn state_with_tool(name: &str, d: f64) -> AppState {
    let mut s = AppState::new(Some(scratch(name).join("tools.json")));
    assert!(s.library_add(Tool {
        diameter: d,
        stepover_pct: 40.0,
        ..tool(1)
    }));
    s
}

fn add_text(s: &mut AppState, t: &str, font: Font, h: f64, at: Point) -> Vec<u64> {
    s.add_shape(Shape::Text {
        at,
        text: t.into(),
        font,
        height: h,
    })
    .unwrap()
}

fn contours(s: &AppState, ids: &[u64]) -> Vec<Contour> {
    ids.iter()
        .map(|&id| match &s.item(id).unwrap().geom {
            Geom::Contour(c) => c.clone(),
            Geom::Point(_) => panic!("point"),
        })
        .collect()
}

/// Every XY sample of the feed moves below Z0.
fn cut_samples(s: &AppState, op: u64) -> Vec<Point> {
    let tp = s.toolpath_result(op).unwrap().as_ref().unwrap();
    let mut cur = p(0.0, 0.0);
    let mut out = vec![];
    for m in &tp.moves {
        match *m {
            Move::Rapid(to) => cur = to.xy(),
            Move::Linear { to, .. } => {
                let n = (cur.distance(to.xy()) / 0.05).ceil().max(1.0) as usize;
                if to.z < -1e-6 {
                    for k in 0..=n {
                        let t = k as f64 / n as f64;
                        out.push(cur + (to.xy() - cur) * t);
                    }
                }
                cur = to.xy();
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                let a0 = (cur.y - center.y).atan2(cur.x - center.x);
                let mut a1 = (to.y - center.y).atan2(to.x - center.x);
                if ccw && a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                if !ccw && a1 >= a0 {
                    a1 -= std::f64::consts::TAU;
                }
                let rad = center.distance(cur);
                let n = ((a1 - a0).abs() * rad / 0.05).ceil().max(1.0) as usize;
                if to.z < -1e-6 {
                    for k in 0..=n {
                        let a = a0 + (a1 - a0) * k as f64 / n as f64;
                        out.push(center + p(rad * a.cos(), rad * a.sin()));
                    }
                }
                cur = to.xy();
            }
            Move::Drill { .. } => {}
        }
    }
    out
}

fn ink(cs: &[Contour], q: Point) -> bool {
    cs.iter().filter(|c| c.contains_point(q)).count() % 2 == 1
}

fn edge_dist(cs: &[Contour], q: Point) -> f64 {
    cs.iter()
        .map(|c| c.distance_to(q))
        .fold(f64::INFINITY, f64::min)
}

/// Deepest intrusion of the tool edge (radius r) out of the ink (pocket) or into the ink (outside cut).
fn worst(cs: &[Contour], pts: &[Point], r: f64, keep_inside: bool) -> (f64, Point) {
    pts.iter()
        .map(|&q| {
            let d = edge_dist(cs, q);
            let g = if ink(cs, q) == keep_inside {
                r - d
            } else {
                r + d
            };
            (g, q)
        })
        .fold((f64::NEG_INFINITY, p(0.0, 0.0)), |a, b| {
            if b.0 > a.0 {
                b
            } else {
                a
            }
        })
}

fn op_with(s: &mut AppState, ids: &[u64], kind: NewKind) -> u64 {
    s.selection = ids.iter().copied().collect();
    s.new_operation(kind, 1).unwrap()
}

// ---------------------------------------------------------------- cutting text

#[test]
fn pocket_on_outline_text_stays_inside_the_letters() {
    let d = 0.5;
    let mut s = state_with_tool("m10b_pocket", d);
    let ids = add_text(&mut s, "HOBBY 8@%&g", Font::Outline, 10.0, p(0.0, 0.0));
    let cs = contours(&s, &ids);
    let op = op_with(&mut s, &ids, NewKind::Pocket);
    s.refresh_toolpaths();
    assert!(
        s.toolpath_result(op).unwrap().is_ok(),
        "{:?}",
        s.toolpath_result(op)
    );
    let pts = cut_samples(&s, op);
    assert!(pts.len() > 1000);
    let (g, at) = worst(&cs, &pts, d / 2.0, true);
    assert!(g <= TOL, "pocket gouges {g} mm at {at:?}");
    // every counter is left standing: no sample inside a hole
    for hole in cs
        .iter()
        .filter(|c| c.orientation() == Some(Orientation::Cw))
    {
        assert!(pts.iter().all(|&q| !hole.contains_point(q) || !ink(&cs, q)));
    }
    assert!(!s
        .issues
        .iter()
        .any(|i| i.op == Some(op) && i.severity == hc_toolpath::Severity::Error));
}

#[test]
fn pocket_too_large_tool_is_a_clear_error_not_a_silent_cut() {
    let mut s = state_with_tool("m10b_big", 3.175);
    let ids = add_text(&mut s, "HO", Font::Outline, 10.0, p(0.0, 0.0));
    let op = op_with(&mut s, &ids, NewKind::Pocket);
    s.refresh_toolpaths();
    assert!(matches!(
        s.toolpath_result(op).unwrap(),
        Err(ToolpathError::OffsetVanished(_))
    ));
    assert!(s.issues.iter().any(|i| i.op == Some(op)
        && i.rule == RuleId::ToolTooLarge
        && i.severity == hc_toolpath::Severity::Error));
}

/// A tool wider than the thin parts of a stroke but narrower than the thick ones: whatever is generated must not gouge,
/// and when a glyph is cut only partly the user must hear about it.
#[test]
fn pocket_tool_between_stroke_widths_never_gouges() {
    for d in [0.5, 0.6, 0.8, 0.9, 1.0, 1.1, 1.2] {
        let mut s = state_with_tool("m10b_mid", d);
        let ids = add_text(&mut s, "HOBe", Font::Outline, 10.0, p(0.0, 0.0));
        let cs = contours(&s, &ids);
        let op = op_with(&mut s, &ids, NewKind::Pocket);
        s.refresh_toolpaths();
        if s.toolpath_result(op).unwrap().is_ok() {
            let pts = cut_samples(&s, op);
            let (g, at) = worst(&cs, &pts, d / 2.0, true);
            assert!(g <= TOL, "Ø{d}: gouge {g} at {at:?}");
            if d <= 0.6 {
                assert_eq!(
                    uncut_core(&cs, &pts, d / 2.0),
                    (0, None),
                    "Ø{d}: letter cores left uncut"
                );
            }
        }
    }
}

/// Ubuntu Light strokes of 10 mm text are ~0.9 mm wide. With Ø1.0 the pocket is Ok and silent, but large parts of every
/// letter are never cut (the ring offset vanishes only locally, OffsetVanished needs it to vanish everywhere).
#[test]
fn pocket_tool_wider_than_a_stroke_is_reported() {
    let d = 1.0;
    let mut s = state_with_tool("m10b_partial", d);
    let ids = add_text(&mut s, "HOBe", Font::Outline, 10.0, p(0.0, 0.0));
    let cs = contours(&s, &ids);
    let op = op_with(&mut s, &ids, NewKind::Pocket);
    s.refresh_toolpaths();
    let ok = s.toolpath_result(op).unwrap().is_ok();
    let uncut = if ok {
        uncut_core(&cs, &cut_samples(&s, op), d / 2.0).0
    } else {
        0
    };
    let reported = s.issues.iter().any(|i| i.op == Some(op));
    assert!(
        uncut == 0 || reported,
        "{uncut} grid points (0.1 mm) of letter core never cut, no issue"
    );
}

/// Ink points at least 0.2 mm from the edge that no tool position covers: (count, first), on a 0.1 mm grid.
fn uncut_core(cs: &[Contour], pts: &[Point], r: f64) -> (usize, Option<Point>) {
    let cell = r;
    let key = |q: Point| ((q.x / cell).floor() as i64, (q.y / cell).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<Point>> = Default::default();
    for &q in pts {
        grid.entry(key(q)).or_default().push(q);
    }
    let b = cs
        .iter()
        .fold(hc_geom::BBox::empty(), |b, c| b.union(&c.bbox()));
    let (mut n, mut first) = (0, None);
    let mut y = b.min.y;
    while y < b.max.y {
        let mut x = b.min.x;
        while x < b.max.x {
            let q = p(x, y);
            if ink(cs, q) && edge_dist(cs, q) >= 0.2 {
                let (kx, ky) = key(q);
                let hit = (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        grid.get(&(kx + dx, ky + dy))
                            .is_some_and(|v| v.iter().any(|t| t.distance(q) <= r + 0.05))
                    })
                });
                if !hit {
                    n += 1;
                    first.get_or_insert(q);
                }
            }
            x += 0.1;
        }
        y += 0.1;
    }
    (n, first)
}

#[test]
fn contour_outside_with_tabs_on_outer_letter_contours_does_not_touch_the_letters() {
    let d = 1.0;
    let mut s = state_with_tool("m10b_outside", d);
    let ids = add_text(&mut s, "HOBO", Font::Outline, 20.0, p(0.0, 0.0));
    let cs = contours(&s, &ids);
    let outers: Vec<u64> = ids
        .iter()
        .zip(&cs)
        .filter(|(_, c)| c.orientation() == Some(Orientation::Ccw))
        .map(|(&id, _)| id)
        .collect();
    let op = op_with(&mut s, &outers, NewKind::Contour);
    edit_op(&mut s, op, |o| {
        o.kind = OpKind::Contour {
            side: Side::Outside,
            direction: Direction::Climb,
        };
        o.depth = 3.0;
        o.tabs.enabled = true;
    });
    s.refresh_toolpaths();
    let tp = s.toolpath_result(op).unwrap().as_ref().unwrap();
    // tabs: some feeds at the full depth and some lifted above it
    let zs: Vec<f64> = tp
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Linear { to, .. } | Move::Arc { to, .. } if to.z < 0.0 => Some(to.z),
            _ => None,
        })
        .collect();
    assert!(zs.iter().any(|&z| (z + 3.0).abs() < 1e-9));
    assert!(zs.iter().any(|&z| z > -3.0 + 1e-6), "no tab lift");
    let (g, at) = worst(&cs, &cut_samples(&s, op), d / 2.0, false);
    assert!(g <= TOL, "outside cut enters a letter by {g} at {at:?}");
}

/// Selecting the whole text (outers and counters) for Contour Outside is the obvious user action. Each contour is offset
/// on its own, so the counter of O grows into the O's ring.
#[test]
fn contour_outside_on_a_whole_text_selection_does_not_cut_into_the_letters() {
    let d = 1.0;
    let mut s = state_with_tool("m10b_outside_all", d);
    let ids = add_text(&mut s, "O", Font::Outline, 20.0, p(0.0, 0.0));
    let cs = contours(&s, &ids);
    let op = op_with(&mut s, &ids, NewKind::Contour);
    edit_op(&mut s, op, |o| {
        o.kind = OpKind::Contour {
            side: Side::Outside,
            direction: Direction::Climb,
        };
    });
    s.refresh_toolpaths();
    let gouge = match s.toolpath_result(op).unwrap() {
        Ok(_) => worst(&cs, &cut_samples(&s, op), d / 2.0, false).0,
        Err(_) => f64::NEG_INFINITY,
    };
    // OutsideStock is unrelated (the stock is the default 100 x 100 at 0,0)
    let warned = s
        .issues
        .iter()
        .any(|i| i.op == Some(op) && i.rule != RuleId::OutsideStock);
    assert!(
        gouge <= TOL || warned,
        "O ring cut by {gouge} mm without any issue"
    );
}

#[test]
fn engrave_on_hershey_text_follows_the_strokes() {
    let mut s = state_with_tool("m10b_engrave", 0.5);
    let ids = add_text(
        &mut s,
        "Hello, World! 0123 @&",
        Font::Hershey,
        8.0,
        p(10.0, 10.0),
    );
    let cs = contours(&s, &ids);
    let op = op_with(&mut s, &ids, NewKind::Engrave);
    s.refresh_toolpaths();
    let pts = cut_samples(&s, op);
    assert!(!pts.is_empty());
    // the cutter centre stays on the strokes (Engrave = on the line)
    let off = pts.iter().map(|&q| edge_dist(&cs, q)).fold(0.0, f64::max);
    assert!(off < 1e-6, "engrave leaves the strokes by {off}");
    // and every stroke is cut: each stroke's midpoint is visited
    for c in &cs {
        let m = c.point_at(c.length() / 2.0);
        assert!(
            pts.iter().any(|q| q.distance(m) < 0.05),
            "stroke at {m:?} not cut"
        );
    }
    assert!(!s
        .issues
        .iter()
        .any(|i| i.op == Some(op) && i.severity == hc_toolpath::Severity::Error));
}

// ---------------------------------------------------------------- height, units, placement

#[test]
fn text_height_is_mm_internally_in_inch_projects() {
    let mut s = AppState::new(None);
    s.set_units(Units::Inch);
    let ids = add_text(&mut s, "H", Font::Outline, 25.4, p(0.0, 0.0));
    let b = contours(&s, &ids)[0].bbox();
    assert!((b.height() - 25.4).abs() < 1e-9);
    // error text is unit-free
    for h in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            s.add_shape(Shape::Text {
                at: p(0.0, 0.0),
                text: "H".into(),
                font: Font::Hershey,
                height: h,
            }),
            Err("Text needs a height".into()),
            "{h}"
        );
    }
}

#[test]
fn very_small_and_very_large_text_are_added() {
    let mut s = AppState::new(None);
    for (h, font) in [
        (0.5, Font::Hershey),
        (0.5, Font::Outline),
        (500.0, Font::Outline),
    ] {
        let ids = add_text(&mut s, "Hg8", font, h, p(0.0, 0.0));
        let cs = contours(&s, &ids);
        let b = cs[0].bbox(); // H's first contour/stroke spans the cap height (stem)
        assert!(b.height() <= h + 1e-9, "{h}");
        assert!(cs.iter().all(|c| c
            .segments
            .iter()
            .all(|x| x.start().x.is_finite() && x.end().y.is_finite())));
    }
}

#[test]
fn text_click_uses_the_given_point_as_baseline_start() {
    let mut s = AppState::new(None);
    s.set_tool(CanvasTool::Text);
    s.draw.text = "H".into();
    s.draw.font = Font::Outline;
    s.canvas_click(p(12.5, -7.25), false);
    let ids: Vec<u64> = s.project.items.iter().map(|i| i.id).collect();
    let b = contours(&s, &ids)[0].bbox();
    assert!((b.min.y + 7.25).abs() < 1e-9);
    // the H outline starts at its left side bearing, so min.x is just right of the click
    assert!(b.min.x > 12.5 && b.min.x < 12.5 + 2.0);
    // the tool stays Text for the next click; a second click adds a second text
    assert_eq!(s.tool, CanvasTool::Text);
    s.canvas_click(p(30.0, 0.0), false);
    assert_eq!(s.project.items.len(), 2 * ids.len());
}

#[test]
fn mirrored_outline_text_keeps_holes_as_islands() {
    let mut s = state_with_tool("m10b_mirror", 0.5);
    let ids = add_text(&mut s, "BO", Font::Outline, 10.0, p(0.0, 0.0));
    for (name, t) in [
        ("mirror", Transform::mirror_y(5.0)),
        ("rotate", Transform::rotate(1.0, p(3.0, 3.0))),
    ] {
        s.selection = ids.iter().copied().collect();
        s.transform_selection(&t);
        let cs = contours(&s, &ids);
        let tree = hc_geom::containment_tree(&cs);
        let holes = tree.iter().filter(|n| n.depth == 1).count();
        assert_eq!(holes, 3, "{name}: B has 2 counters, O 1");
        // a pocket on it still leaves the counters
        let op = op_with(&mut s, &ids, NewKind::Pocket);
        s.refresh_toolpaths();
        assert!(s.toolpath_result(op).unwrap().is_ok(), "{name}");
        let (g, at) = worst(&cs, &cut_samples(&s, op), 0.25, true);
        assert!(g <= TOL, "{name}: gouge {g} at {at:?}");
        s.delete_op(op);
    }
}

#[test]
fn mirrored_text_reads_mirrored() {
    // mirror is a user action: the L foot must now point left (sanity that transform really mirrors text geometry)
    let mut s = AppState::new(None);
    let ids = add_text(&mut s, "L", Font::Hershey, 10.0, p(0.0, 0.0));
    s.selection = ids.iter().copied().collect();
    s.transform_selection(&Transform::mirror_y(0.0));
    let cs = contours(&s, &ids);
    let b = cs
        .iter()
        .fold(hc_geom::BBox::empty(), |b, c| b.union(&c.bbox()));
    assert!(b.max.x <= 1e-9, "mirrored about x = 0 lies left of it");
}

// ---------------------------------------------------------------- warnings

#[test]
fn unicode_input_warning_texts() {
    let cases: [(&str, Font, Option<&str>); 9] = [
        ("Hé", Font::Hershey, Some("Not in the font: é")),
        ("Hé", Font::Outline, None),
        ("Árvíztűrő", Font::Outline, None),
        ("a😀b", Font::Outline, Some("Not in the font: U+1F600")),
        (
            "中文",
            Font::Outline,
            Some("Not in the font: U+4E2D, U+6587"),
        ),
        (
            "a\tb\nc",
            Font::Outline,
            Some("Not in the font: U+0009, U+000A"),
        ),
        ("e\u{301}", Font::Hershey, Some("Not in the font: U+0301")),
        ("aaa😀😀", Font::Hershey, Some("Not in the font: U+1F600")),
        ("שלום", Font::Hershey, None), // checked below: Hebrew is shown or coded, never a panic
    ];
    for (t, font, want) in cases {
        let mut s = AppState::new(None);
        let before = s.messages.len();
        let _ = s.add_shape(Shape::Text {
            at: p(0.0, 0.0),
            text: t.into(),
            font,
            height: 5.0,
        });
        let new: Vec<_> = s.messages[before..].to_vec();
        match want {
            Some(w) => {
                assert_eq!(new.len(), 1, "{t:?}: {new:?}");
                assert_eq!(new[0].severity, Severity::Warning);
                assert_eq!(new[0].text, w, "{t:?}");
            }
            None if t == "שלום" => {
                assert_eq!(new.len(), 1);
                assert!(new[0].text.starts_with("Not in the font: "));
            }
            None => assert!(new.is_empty(), "{t:?}: {new:?}"),
        }
    }
}

#[test]
fn text_of_only_missing_characters_adds_nothing_but_warns() {
    let mut s = AppState::new(None);
    let r = s.add_shape(Shape::Text {
        at: p(0.0, 0.0),
        text: "中".into(),
        font: Font::Outline,
        height: 10.0,
    });
    assert_eq!(r, Ok(vec![]));
    assert!(s.project.items.is_empty() && !s.dirty);
    assert_eq!(s.messages.last().unwrap().severity, Severity::Warning);
}

// ---------------------------------------------------------------- undo, save/load

#[test]
fn undo_redo_of_a_text_add_is_one_step() {
    let mut s = AppState::new(None);
    s.commit_undo(true);
    let before = s.project.clone();
    let ids = add_text(&mut s, "HOBBY", Font::Outline, 10.0, p(0.0, 0.0));
    assert!(ids.len() > 5);
    s.commit_undo(true);
    let after = s.project.clone();
    assert!(s.undo());
    assert_eq!(s.project, before);
    assert!(s.selection.is_empty());
    assert!(!s.can_undo(), "one text = one undo step");
    assert!(s.redo());
    assert_eq!(s.project, after);
}

#[test]
fn save_load_round_trip_stores_text_as_plain_contours() {
    let dir = scratch("m10b_save");
    let path = dir.join("text.hcam");
    let mut s = AppState::new(None);
    add_text(&mut s, "HO", Font::Outline, 10.0, p(1.0, 2.0));
    add_text(&mut s, "HI", Font::Hershey, 5.0, p(0.0, 20.0));
    assert!(s.save_as(&path));
    let json = std::fs::read_to_string(&path).unwrap();
    assert!(
        json.contains("\"format_version\": 4"),
        "format unchanged (v4)"
    );
    assert!(!json.contains("Hershey") && !json.contains("Ubuntu") && !json.contains("\"text\""));
    let mut t = AppState::new(None);
    assert!(t.open(&path));
    assert_eq!(
        t.project.items, s.project.items,
        "bit-exact geometry after reload"
    );
}

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

    #[test]
    fn text_click_snaps_the_baseline_to_a_corner() {
        let mut s = AppState::new(None);
        s.add_shape(Shape::Rect {
            min: p(0.0, 0.0),
            w: 40.0,
            h: 30.0,
            r: 0.0,
        })
        .unwrap();
        s.clear_selection();
        s.set_tool(CanvasTool::Text);
        s.draw.text = "H".into();
        s.draw.font = Font::Outline;
        let mut h = harness(s);
        h.run();
        // screen position of the corner (40, 30) via a hover probe
        let probe = egui::pos2(640.0, 400.0);
        h.hover_at(probe);
        h.run();
        let (c, z) = {
            let s = &h.state().state;
            (s.cursor.unwrap(), s.view.zoom)
        };
        let at = probe
            + egui::vec2(((40.0 - c.x) as f32) * z, -((30.0 - c.y) as f32) * z)
            + egui::vec2(3.0, -2.0);
        h.hover_at(at);
        h.run();
        h.drag_at(at);
        h.run();
        h.drop_at(at);
        h.run();
        let s = &h.state().state;
        assert_eq!(s.draw.a, p(40.0, 30.0), "baseline start snapped exactly");
        assert_eq!(s.project.items.len(), 2);
        let Geom::Contour(k) = &s.project.items[1].geom else {
            panic!()
        };
        assert!((k.bbox().min.y - 30.0).abs() < 1e-9);
    }

    #[test]
    fn text_height_field_shows_inches_in_an_inch_project() {
        let mut s = AppState::new(None);
        s.set_units(Units::Inch);
        s.set_tool(CanvasTool::Text);
        s.draw.height = 12.7;
        let mut h = harness(s);
        h.run();
        assert!(h.query_by_label("Draw text").is_some());
        // DragValue: 12.7 mm = 0.5 in
        let n = h.get_by_label("Height");
        let node = n.accesskit_node();
        let shown = node.numeric_value();
        assert_eq!(shown, Some(0.5), "{:?} / {:?}", node.value(), shown);
    }
}
