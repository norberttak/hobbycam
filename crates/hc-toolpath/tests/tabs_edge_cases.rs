//! M6 edge cases (tester): holding tabs. Tabbed toolpaths are checked against the same op without tabs
//! (continuity, no extra rapids, vertical lifts at plunge feed, lifted moves exactly at z_tab, arc radii kept,
//! same XY length), plus placement caps, overlaps, loop start, manual positions and validation.
//! Machine safe 5, clearance 2; T1 flat Ø6; R = 100×50 rectangle, CCW, start (0,0), length 300.

use hc_geom::{Circle, Contour, Point, Segment};
use hc_project::{
    Direction, Geom, Item, ManualTabs, OpKind, Operation, Project, Side, Source, TabPlacement,
    Tabs, Tool, ToolKind,
};
use hc_toolpath::*;
use std::f64::consts::{PI, TAU};

const T: f64 = 1e-6;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn tool(id: u32, d: f64) -> Tool {
    Tool {
        id,
        name: format!("Flat {id}"),
        kind: ToolKind::FlatEndMill,
        diameter: d,
        flute_length: 20.0,
        flutes: 2,
        feed: 800.0,
        plunge_feed: 200.0,
        rpm: 18000,
        depth_per_pass: 2.0,
        stepover_pct: 50.0,
        comment: String::new(),
    }
}

fn poly(v: &[(f64, f64, f64)], closed: bool) -> Contour {
    let v: Vec<(Point, f64)> = v.iter().map(|&(x, y, b)| (p(x, y), b)).collect();
    Contour::from_bulge_vertices(&v, closed)
}

fn rect(x0: f64, y0: f64, w: f64, h: f64) -> Contour {
    poly(
        &[
            (x0, y0, 0.0),
            (x0 + w, y0, 0.0),
            (x0 + w, y0 + h, 0.0),
            (x0, y0 + h, 0.0),
        ],
        true,
    )
}

fn r_rect() -> Contour {
    rect(0.0, 0.0, 100.0, 50.0)
}

/// 60×40 rectangle with corner radius 5, CCW, start (5,0).
fn rounded() -> Contour {
    let b = (PI / 8.0).tan();
    poly(
        &[
            (5.0, 0.0, 0.0),
            (55.0, 0.0, b),
            (60.0, 5.0, 0.0),
            (60.0, 35.0, b),
            (55.0, 40.0, 0.0),
            (5.0, 40.0, b),
            (0.0, 35.0, 0.0),
            (0.0, 5.0, b),
        ],
        true,
    )
}

fn circle(r: f64) -> Contour {
    Circle {
        center: p(0.0, 0.0),
        radius: r,
    }
    .to_contour()
}

fn add_c(pr: &mut Project, c: Contour) -> u64 {
    let id = pr.next_id;
    pr.next_id += 1;
    pr.items.push(Item {
        id,
        layer: "0".into(),
        source: Source::Drawn,
        geom: Geom::Contour(c),
    });
    id
}

fn tabs(placement: TabPlacement) -> Tabs {
    Tabs {
        enabled: true,
        placement,
        width: 5.0,
        height: 2.0,
        manual: Vec::new(),
    }
}

fn count(n: u32) -> Tabs {
    tabs(TabPlacement::Count { count: n })
}

fn manual(item: u64, positions: &[f64]) -> Tabs {
    Tabs {
        manual: vec![ManualTabs {
            item,
            positions: positions.to_vec(),
        }],
        ..count(4)
    }
}

/// Project with tool Ø`d`, item `c`, one op `kind` depth 6 dpp 2 with `t`. (project, item, op)
fn setup(c: Contour, kind: OpKind, d: f64, t: Tabs) -> (Project, u64, u64) {
    let mut pr = Project::new();
    pr.tools.push(tool(1, d));
    let id = add_c(&mut pr, c);
    let tl = pr.tools[0].clone();
    let op = pr.add_operation("op", kind.clone(), &tl, vec![id]);
    let o = op_mut(&mut pr, op);
    o.kind = kind;
    o.depth = 6.0;
    o.depth_per_pass = 2.0;
    o.tabs = t;
    (pr, id, op)
}

fn outside() -> OpKind {
    OpKind::Contour {
        side: Side::Outside,
        direction: Direction::Climb,
    }
}

fn inside() -> OpKind {
    OpKind::Contour {
        side: Side::Inside,
        direction: Direction::Climb,
    }
}

fn on() -> OpKind {
    OpKind::Contour {
        side: Side::On,
        direction: Direction::Climb,
    }
}

fn op_mut(pr: &mut Project, id: u64) -> &mut Operation {
    pr.operations.iter_mut().find(|o| o.id == id).unwrap()
}

fn op_ref(pr: &Project, id: u64) -> &Operation {
    pr.operations.iter().find(|o| o.id == id).unwrap()
}

fn run(pr: &Project, id: u64) -> Result<Toolpath, ToolpathError> {
    generate_op(pr, op_ref(pr, id))
}

fn arc_sweep(from: Point, to: Point, center: Point, ccw: bool) -> f64 {
    let a0 = (from.y - center.y).atan2(from.x - center.x);
    let a1 = (to.y - center.y).atan2(to.x - center.x);
    if ccw {
        (a1 - a0).rem_euclid(TAU)
    } else {
        -(a0 - a1).rem_euclid(TAU)
    }
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < T
}

/// M5 test 10 invariants (copied from acceptance.rs).
fn check_invariants(pr: &Project, tp: &Toolpath) {
    let m = &pr.machine;
    let op = op_ref(pr, tp.op_id);
    let Move::Rapid(first) = tp.moves[0] else {
        panic!("first move must be a rapid")
    };
    assert!((first.z - m.safe_z).abs() < T, "first move at safe_z");
    let mut pos = first;
    for mv in &tp.moves[1..] {
        match mv {
            Move::Rapid(to) => {
                let xy = !to.xy().approx_eq(pos.xy(), 1e-9);
                let z = (to.z - pos.z).abs() > 1e-9;
                assert!(!(xy && z), "rapid changes XY and Z: {pos:?} -> {to:?}");
                assert!(to.z >= m.clearance_z - 1e-9, "rapid ends below clearance");
                if xy {
                    assert!(pos.z >= m.clearance_z - 1e-9, "XY rapid below clearance");
                }
                pos = *to;
            }
            Move::Linear { to, feed } => {
                if to.z < pos.z - 1e-9 {
                    assert!(to.xy().approx_eq(pos.xy(), 1e-9), "plunge is vertical");
                    assert!((feed - op.plunge_feed).abs() < T);
                } else if !to.xy().approx_eq(pos.xy(), 1e-9) && to.z < 0.0 {
                    assert!((feed - op.feed).abs() < T);
                    assert!((to.z - pos.z).abs() < 1e-9);
                }
                pos = *to;
            }
            Move::Arc {
                to,
                center,
                ccw,
                feed,
            } => {
                assert!((feed - op.feed).abs() < T);
                assert!((to.z - pos.z).abs() < 1e-9);
                let sweep = arc_sweep(pos.xy(), to.xy(), *center, *ccw);
                assert!(sweep.abs() <= PI + 1e-9, "sweep {sweep}");
                assert!((pos.xy().distance(*center) - to.xy().distance(*center)).abs() < T);
                pos = *to;
            }
            Move::Drill { .. } => unreachable!(),
        }
        assert!(pos.z >= -op.depth - 1e-9, "below final depth");
    }
    assert!((pos.z - m.safe_z).abs() < T, "ends at safe_z");
}

#[derive(Debug)]
struct Lift {
    level: f64,
    path: Vec<Segment>,
    all_linear: bool,
}

impl Lift {
    fn len(&self) -> f64 {
        self.path.iter().map(Segment::length).sum()
    }
    fn mid(&self) -> Point {
        Contour {
            segments: self.path.clone(),
            closed: false,
        }
        .point_at(self.len() / 2.0)
    }
}

fn rapids(tp: &Toolpath) -> usize {
    tp.moves
        .iter()
        .filter(|m| matches!(m, Move::Rapid(_)))
        .count()
}

/// (center, radius) of every arc move.
fn arcs(tp: &Toolpath) -> Vec<(Point, f64)> {
    let mut out = Vec::new();
    let mut pos = Point::new(f64::NAN, f64::NAN);
    for m in &tp.moves {
        match m {
            Move::Rapid(to) | Move::Linear { to, .. } => pos = to.xy(),
            Move::Arc { to, center, .. } => {
                out.push((*center, pos.distance(*center)));
                pos = to.xy();
            }
            Move::Drill { .. } => {}
        }
    }
    out
}

/// Full tab check of `tp` against the same op with tabs disabled; returns the lifts.
fn check_tabbed(pr: &Project, tp: &Toolpath) -> Vec<Lift> {
    check_invariants(pr, tp);
    let op = op_ref(pr, tp.op_id);
    let mut plain_pr = pr.clone();
    op_mut(&mut plain_pr, op.id).tabs.enabled = false;
    let plain = run(&plain_pr, op.id).unwrap();
    assert_eq!(rapids(tp), rapids(&plain), "tabs must not add rapids");

    let levels = depth_levels(op.depth, op.depth_per_pass);
    let at_level = |z: f64| levels.iter().any(|&l| (l - z).abs() < 1e-9);
    let z_tab = (-op.depth + op.tabs.height).min(0.0); // M6 review R2: clamped to Z0
    let plain_arcs = arcs(&plain);
    let mut lifts: Vec<Lift> = Vec::new();
    let mut lifted = false;
    let mut cur_level = f64::NAN;
    let mut pos = match tp.moves[0] {
        Move::Rapid(p) => p,
        _ => unreachable!(),
    };
    let mut xy_len = 0.0;
    for m in &tp.moves[1..] {
        let from = pos;
        let (to, seg) = match m {
            Move::Rapid(to) => {
                assert!(!lifted, "rapid during a lift");
                (*to, None)
            }
            Move::Linear { to, feed } => {
                let vertical = to.xy().approx_eq(from.xy(), 1e-9);
                assert!(
                    !(vertical && (to.z - from.z).abs() < 1e-12),
                    "zero-length move at {to:?}"
                );
                if to.z > from.z + 1e-9 {
                    // only tabs raise the tool with a feed move
                    assert!(vertical && approx(*feed, op.plunge_feed), "lift up {m:?}");
                    assert!(
                        !lifted && approx(to.z, z_tab),
                        "lift to {to:?}, z_tab {z_tab}"
                    );
                    assert!(at_level(from.z) && from.z < z_tab - 1e-9);
                    lifted = true;
                    lifts.push(Lift {
                        level: from.z,
                        path: Vec::new(),
                        all_linear: true,
                    });
                    (*to, None)
                } else if to.z < from.z - 1e-9 {
                    assert!(vertical && approx(*feed, op.plunge_feed), "down {m:?}");
                    if lifted {
                        assert!(approx(to.z, cur_level), "lift ends at its pass Z");
                        lifted = false;
                    } else {
                        assert!(at_level(to.z));
                        cur_level = to.z;
                    }
                    (*to, None)
                } else {
                    (
                        *to,
                        Some(Segment::Line {
                            start: from.xy(),
                            end: to.xy(),
                        }),
                    )
                }
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                let d = from.xy() - *center;
                let r = d.length();
                assert!(
                    plain_arcs
                        .iter()
                        .any(|(c, pr)| c.approx_eq(*center, T) && approx(*pr, r)),
                    "arc radius/center changed: {center:?} r {r}"
                );
                if lifted {
                    lifts.last_mut().unwrap().all_linear = false;
                }
                (
                    *to,
                    Some(Segment::Arc {
                        center: *center,
                        radius: r,
                        start_angle: d.y.atan2(d.x),
                        sweep: arc_sweep(from.xy(), to.xy(), *center, *ccw),
                    }),
                )
            }
            Move::Drill { .. } => unreachable!(),
        };
        if let Some(seg) = seg {
            assert!(seg.length() > 1e-7, "tiny move {seg:?}");
            if lifted {
                assert!(approx(to.z, z_tab), "lifted move not at z_tab: {to:?}");
                lifts.last_mut().unwrap().path.push(seg);
            } else {
                assert!(at_level(to.z), "XY feed move off a pass Z: {to:?}");
            }
            xy_len += seg.length();
        }
        pos = to;
    }
    assert!(!lifted, "ends lifted");
    let plain_len: f64 = cut_len(&plain);
    assert!(
        (xy_len - plain_len).abs() < 1e-6,
        "XY length {xy_len} vs {plain_len}"
    );
    for l in &lifts {
        assert!(l.len() > 0.0);
        assert!(z_tab <= pr.machine.clearance_z, "lift above clearance");
    }
    lifts
}

fn cut_len(tp: &Toolpath) -> f64 {
    let mut pos = Point::new(f64::NAN, f64::NAN);
    let mut z = f64::NAN;
    let mut total = 0.0;
    for m in &tp.moves {
        match m {
            Move::Rapid(to) => {
                pos = to.xy();
                z = to.z;
            }
            Move::Linear { to, .. } => {
                if (to.z - z).abs() < 1e-12 {
                    total += pos.distance(to.xy());
                }
                pos = to.xy();
                z = to.z;
            }
            Move::Arc {
                to, center, ccw, ..
            } => {
                total += pos.distance(*center) * arc_sweep(pos, to.xy(), *center, *ccw).abs();
                pos = to.xy();
                z = to.z;
            }
            Move::Drill { .. } => {}
        }
    }
    total
}

fn plunge_xy(tp: &Toolpath) -> Point {
    match tp.moves[1] {
        Move::Rapid(p) => p.xy(),
        _ => panic!("moves[1] must be the XY rapid"),
    }
}

// ---------------------------------------------------------------- invariants on many tabbed toolpaths

#[test]
fn invariants_all_shapes_sides_directions() {
    let shapes = [
        r_rect(),
        rounded(),
        circle(30.0),
        rect(-20.0, 5.0, 33.0, 71.0),
    ];
    for (si, shape) in shapes.iter().enumerate() {
        for side in [Side::Outside, Side::Inside, Side::On] {
            for dir in [Direction::Climb, Direction::Conventional] {
                for t in [
                    count(1),
                    count(3),
                    count(4),
                    count(7),
                    tabs(TabPlacement::Spacing { distance: 25.0 }),
                ] {
                    let kind = OpKind::Contour {
                        side,
                        direction: dir,
                    };
                    let (pr, _, op) = setup(shape.clone(), kind, 6.0, t.clone());
                    let tp = run(&pr, op).unwrap();
                    let lifts = check_tabbed(&pr, &tp);
                    // only the final pass (−6) is below z_tab −4
                    assert!(lifts.iter().all(|l| approx(l.level, -6.0)));
                    // each lift is ≥ w + D unless merged (then longer); never shorter
                    for l in &lifts {
                        assert!(
                            l.len() >= 11.0 - 1e-6 || si == 2,
                            "shape {si} {side:?} {dir:?} {t:?}: lift {}",
                            l.len()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn invariants_dpp_not_dividing_depth() {
    // depth 5, dpp 2 → levels −5/3, −10/3, −5; h 2 → z_tab −3: two passes lift
    let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
    op_mut(&mut pr, op).depth = 5.0;
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 8);
    assert_eq!(lifts.iter().filter(|l| approx(l.level, -5.0)).count(), 4);
    assert_eq!(
        lifts
            .iter()
            .filter(|l| approx(l.level, -10.0 / 3.0))
            .count(),
        4
    );
}

#[test]
fn multiple_items_with_mixed_tabs() {
    let mut pr = Project::new();
    pr.tools.push(tool(1, 6.0));
    let a = add_c(&mut pr, r_rect());
    let b = add_c(&mut pr, rect(200.0, 0.0, 40.0, 40.0));
    let c = add_c(&mut pr, rect(300.0, 0.0, 4.0, 4.0)); // L 16 < min_pitch 17: no tab
    let tl = pr.tools[0].clone();
    let op = pr.add_operation("op", outside(), &tl, vec![a, b, c]);
    let o = op_mut(&mut pr, op);
    o.depth = 6.0;
    o.depth_per_pass = 2.0;
    o.tabs = Tabs {
        manual: vec![ManualTabs {
            item: b,
            positions: vec![0.125],
        }],
        ..count(4)
    };
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 4 + 1);
    assert_eq!(
        tp.notes,
        [ToolpathNote::TabsReduced {
            item: c,
            placed: 0,
            requested: 4
        }]
    );
    assert_eq!(
        tp.notes[0].to_string(),
        format!("Item {c}: only 0 of 4 tabs fit")
    );
}

// ---------------------------------------------------------------- height and Z boundaries

#[test]
fn height_equal_depth_every_pass_lifts_to_zero() {
    let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
    op_mut(&mut pr, op).tabs.height = 6.0;
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 12, "3 passes × 4 tabs");
}

#[test]
fn height_above_depth_follows_formula() {
    // spec (review R2): z_tab = min(−depth + h, 0), h ≥ depth → every pass lifts to Z0 (M8 warns).
    let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
    op_mut(&mut pr, op).tabs.height = 7.0; // z_tab = min(+1, 0) = 0
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 12);
}

#[test]
fn z_tab_tolerance_boundary() {
    // pass −4 vs z_tab −4 + δ: lifts only if z < z_tab − 1e-9
    for (h, lifts_at_4) in [
        (2.0 + 5e-10, false),
        (2.0 + 1e-8, true),
        (2.0 - 1e-8, false),
    ] {
        let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
        op_mut(&mut pr, op).tabs.height = h;
        let tp = run(&pr, op).unwrap();
        let lifts = check_tabbed(&pr, &tp);
        let n4 = lifts.iter().filter(|l| approx(l.level, -4.0)).count();
        assert_eq!(n4 > 0, lifts_at_4, "h {h}");
    }
}

type Case = (fn(&mut Tabs), &'static str);

#[test]
fn validation_values() {
    let cases: [Case; 10] = [
        (|t| t.width = 0.0, "tab_width"),
        (|t| t.width = -1.0, "tab_width"),
        (|t| t.width = f64::INFINITY, "tab_width"),
        (|t| t.height = 0.0, "tab_height"),
        (|t| t.height = -2.0, "tab_height"),
        (
            |t| t.placement = TabPlacement::Spacing { distance: -5.0 },
            "tab_spacing",
        ),
        (
            |t| t.placement = TabPlacement::Spacing { distance: f64::NAN },
            "tab_spacing",
        ),
        (
            |t| {
                t.manual = vec![ManualTabs {
                    item: 999,
                    positions: vec![f64::NAN],
                }]
            },
            "tabs",
        ),
        (
            |t| {
                t.manual = vec![ManualTabs {
                    item: 1,
                    positions: vec![0.5, f64::INFINITY],
                }]
            },
            "tabs",
        ),
        // width checked before height
        (
            |t| {
                t.width = 0.0;
                t.height = 0.0
            },
            "tab_width",
        ),
    ];
    for (f, want) in cases {
        let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
        f(&mut op_mut(&mut pr, op).tabs);
        assert_eq!(run(&pr, op), Err(ToolpathError::InvalidParam(want)));
        // disabled: not validated
        op_mut(&mut pr, op).tabs.enabled = false;
        assert!(run(&pr, op).is_ok(), "{want}");
        // other op kinds ignore tabs entirely, even invalid ones
        op_mut(&mut pr, op).tabs.enabled = true;
        op_mut(&mut pr, op).kind = OpKind::Engrave;
        assert!(run(&pr, op).is_ok(), "engrave {want}");
    }
    // existing op checks come first
    let (mut pr, _, op) = setup(r_rect(), outside(), 6.0, count(4));
    let o = op_mut(&mut pr, op);
    o.tabs.width = 0.0;
    o.depth = 0.0;
    assert_eq!(run(&pr, op), Err(ToolpathError::InvalidParam("depth")));
}

// ---------------------------------------------------------------- width extremes

#[test]
fn width_larger_than_contour() {
    // auto: min_pitch 412 > 300 → 0 tabs + note, toolpath ok and equal to no tabs
    let (pr, id, op) = setup(
        r_rect(),
        outside(),
        6.0,
        Tabs {
            width: 400.0,
            ..count(4)
        },
    );
    let tp = run(&pr, op).unwrap();
    assert_eq!(
        tp.notes,
        [ToolpathNote::TabsReduced {
            item: id,
            placed: 0,
            requested: 4
        }]
    );
    assert!(check_tabbed(&pr, &tp).is_empty());
    // manual: gap 406 ≥ path length 318.85 → the whole loop would be lifted
    let (pr, id, op) = setup(
        r_rect(),
        outside(),
        6.0,
        Tabs {
            width: 400.0,
            ..manual(1, &[0.5])
        },
    );
    assert_eq!(id, 1);
    assert_eq!(run(&pr, op), Err(ToolpathError::InvalidParam("tabs")));
}

#[test]
fn manual_gap_just_below_full_coverage() {
    // Outside path P = 300 + 6π; w = P − D − 1 → gap P − 1, one 1 mm cut stretch remains
    let big_p = 300.0 + 6.0 * PI;
    let (pr, _, op) = setup(
        r_rect(),
        outside(),
        6.0,
        Tabs {
            width: big_p - 6.0 - 1.0,
            ..manual(1, &[1.0 / 6.0])
        },
    );
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), big_p - 1.0), "{}", lifts[0].len());
    // and the plunge is in the middle of the 1 mm free stretch: (50, −3) is the tab centre,
    // the free stretch is centred opposite to it
    assert!(plunge_xy(&tp).distance(p(50.0, -3.0)) > 50.0);
}

// ---------------------------------------------------------------- auto placement counts

#[test]
fn count_exactly_cap_and_cap_plus_one_on_circle() {
    // circle L = 17·17.5 → cap floor(L/17) = 17; no zones so nominals are kept
    let r = 17.0 * 17.5 / TAU;
    let c = circle(r);
    let (pos, note) = tab_positions(&c, &count(17), 9, 6.0);
    assert_eq!((pos.len(), note), (17, None));
    let (pos, note) = tab_positions(&c, &count(18), 9, 6.0);
    assert_eq!(pos.len(), 17);
    assert_eq!(
        note,
        Some(ToolpathNote::TabsReduced {
            item: 9,
            placed: 17,
            requested: 18
        })
    );
    // L exactly n·min_pitch is still n (floor of an exact multiple)
    let c = circle(4.0 * 17.0 / TAU);
    let (pos, _) = tab_positions(&c, &count(4), 9, 6.0);
    assert_eq!(pos.len(), 4, "L = 4·min_pitch");
}

#[test]
fn count_cap_on_rectangle_keeps_pitch() {
    // R: cap floor(300/17) = 17; snapping may then drop some. note.placed = kept count, pitch ≥ 17.
    for n in [16u32, 17, 18, 100] {
        let (pos, note) = tab_positions(&r_rect(), &count(n), 1, 6.0);
        assert!(pos.len() <= 17);
        if pos.len() < n as usize {
            assert_eq!(
                note,
                Some(ToolpathNote::TabsReduced {
                    item: 1,
                    placed: pos.len(),
                    requested: n as usize
                })
            );
        } else {
            assert_eq!(note, None);
        }
        for (i, a) in pos.iter().enumerate() {
            assert!((0.0..1.0).contains(a));
            for b in &pos[i + 1..] {
                let d = ((a - b).abs() * 300.0).rem_euclid(300.0);
                assert!(d.min(300.0 - d) >= 17.0 - 1e-6, "pitch {a} {b}");
            }
        }
        // all on line zones (≥ 8.5 from corners)
        for a in &pos {
            let s = a * 300.0;
            for corner in [0.0, 100.0, 150.0, 250.0, 300.0] {
                assert!((s - corner).abs() >= 8.5 - 1e-6, "s {s} near corner");
            }
        }
        // the toolpath still works with them
        let (pr, _, op) = setup(r_rect(), outside(), 6.0, count(n));
        check_tabbed(&pr, &run(&pr, op).unwrap());
    }
}

#[test]
fn spacing_values() {
    let n = |d: f64| {
        tab_positions(
            &r_rect(),
            &tabs(TabPlacement::Spacing { distance: d }),
            1,
            6.0,
        )
    };
    assert_eq!(n(300.0).0.len(), 1);
    assert_eq!(n(299.999).0.len(), 2);
    assert_eq!(n(150.0).0.len(), 2);
    assert_eq!(n(1e9).0.len(), 1);
    assert_eq!(n(1e9).1, None);
    // a tiny spacing asks for far more than the cap
    let (pos, note) = n(1.0);
    assert_eq!(
        note,
        Some(ToolpathNote::TabsReduced {
            item: 1,
            placed: pos.len(),
            requested: 300
        })
    );
}

#[test]
fn count_zero_is_no_tabs_no_note() {
    let (pr, _, op) = setup(r_rect(), outside(), 6.0, count(0));
    let tp = run(&pr, op).unwrap();
    assert!(tp.notes.is_empty());
    let mut plain = pr.clone();
    op_mut(&mut plain, op).tabs.enabled = false;
    assert_eq!(tp, run(&plain, op).unwrap());
}

// ---------------------------------------------------------------- loop start and overlaps

#[test]
fn tab_exactly_on_item_start_and_path_start() {
    for t in [0.0, 1.0, -1.0, 2.0] {
        let (pr, _, op) = setup(r_rect(), outside(), 6.0, manual(1, &[t]));
        let tp = run(&pr, op).unwrap();
        let lifts = check_tabbed(&pr, &tp);
        assert_eq!(lifts.len(), 1, "t {t}");
        assert!(approx(lifts[0].len(), 11.0));
        // (0,0) is a corner: nearest path point is on the corner arc (distance 3)
        assert!(approx(lifts[0].mid().distance(p(0.0, 0.0)), 3.0));
        assert!(!lifts[0].all_linear);
        // plunge never inside the lift
        assert!(plunge_xy(&tp).distance(lifts[0].mid()) > 5.5);
    }
}

#[test]
fn tab_on_path_start_on_a_straight() {
    // On side: the path is the item itself, starting at (0,0); a tab at s 2 wraps over the start
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[2.0 / 300.0]));
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 11.0));
    assert!(lifts[0].mid().approx_eq(p(2.0, 0.0), T), "{:?}", lifts[0]);
    // rotated to the middle of the free gap, opposite the tab: s 152 = (98, 50)
    assert!(
        plunge_xy(&tp).approx_eq(p(98.0, 50.0), T),
        "{:?}",
        plunge_xy(&tp)
    );
}

#[test]
fn tab_interval_ending_exactly_at_path_start_is_not_rotated() {
    // On side, tab centre s 294.5, gap 11 → [289, 300]: start is not strictly inside → M5 start kept
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[294.5 / 300.0]));
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 11.0));
    assert!(plunge_xy(&tp).approx_eq(p(0.0, 0.0), T));
    // and starting exactly at s 0
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[5.5 / 300.0]));
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 11.0));
    assert!(plunge_xy(&tp).approx_eq(p(0.0, 0.0), T));
}

#[test]
fn overlapping_and_duplicate_manual_tabs_merge() {
    // s 30 and 33 → [24.5, 38.5]: one lift of 14
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[0.1, 0.11]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 14.0), "{}", lifts[0].len());
    // exact duplicates → one lift of 11
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[0.1, 0.1, 0.1]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 11.0));
    // touching intervals (centres 11 apart) merge into 22
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[0.1, 41.0 / 300.0]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 22.0), "{}", lifts[0].len());
    // overlapping across the start: s 296 and 4 → one lift of 19, plunge outside it
    let (pr, _, op) = setup(
        r_rect(),
        on(),
        6.0,
        manual(1, &[296.0 / 300.0, 4.0 / 300.0]),
    );
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 1);
    assert!(approx(lifts[0].len(), 19.0), "{}", lifts[0].len());
    assert!(lifts[0].mid().approx_eq(p(0.0, 0.0), T));
}

#[test]
fn manual_tab_on_corner_spans_line_and_arc() {
    // Outside, tab at item corner (100,0): lift goes over line + arc + line, arc radius kept (checked)
    let (pr, _, op) = setup(r_rect(), outside(), 6.0, manual(1, &[100.0 / 300.0]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 1);
    assert!(!lifts[0].all_linear);
    assert!(approx(lifts[0].len(), 11.0));
    assert!(lifts[0].path.len() >= 2);
}

#[test]
fn manual_positions_used_rem_euclid() {
    // 1.3 ≡ 0.3, −0.25 ≡ 0.75 (spec: positions are used rem_euclid(1.0))
    let (pr, _, op) = setup(r_rect(), on(), 6.0, manual(1, &[1.3, -0.25]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 2);
    let r = r_rect();
    for want in [r.point_at(90.0), r.point_at(225.0)] {
        assert!(lifts.iter().any(|l| l.mid().approx_eq(want, T)), "{want:?}");
    }
    // tab_positions returns them verbatim (no normalisation)
    let (pos, note) = tab_positions(&r, &manual(1, &[1.3, -0.25]), 1, 6.0);
    assert_eq!((pos, note), (vec![1.3, -0.25], None));
}

#[test]
fn empty_manual_entry_means_no_tabs_and_stale_entries_ignored() {
    let (pr, _, op) = setup(r_rect(), outside(), 6.0, manual(1, &[]));
    let tp = run(&pr, op).unwrap();
    assert!(tp.notes.is_empty());
    assert!(check_tabbed(&pr, &tp).is_empty());
    // an entry for an item not in the op is ignored: auto tabs on item 1
    let (pr, _, op) = setup(r_rect(), outside(), 6.0, manual(42, &[0.5]));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 4);
}

#[test]
fn manual_survives_direction_change() {
    let mut mids = Vec::new();
    for dir in [Direction::Climb, Direction::Conventional] {
        let kind = OpKind::Contour {
            side: Side::Outside,
            direction: dir,
        };
        let (pr, _, op) = setup(r_rect(), kind, 6.0, manual(1, &[1.0 / 6.0, 0.4]));
        let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
        assert_eq!(lifts.len(), 2);
        mids.push(lifts.iter().map(Lift::mid).collect::<Vec<_>>());
    }
    for m in &mids[0] {
        assert!(mids[1].iter().any(|q| q.approx_eq(*m, T)), "{mids:?}");
    }
    assert!(mids[0].iter().any(|q| q.approx_eq(p(50.0, -3.0), T)));
    assert!(mids[0].iter().any(|q| q.approx_eq(p(103.0, 20.0), T)));
}

#[test]
fn corner_tab_projection_is_direction_dependent() {
    // FINDING (spec-undefined): a manual tab exactly on a sharp item corner lies at the centre of the
    // Outside corner arc, so every arc point is nearest. The lift lands on the arc either way, but at a
    // different point for Climb vs Conventional, contradicting "positions survive direction changes".
    let mut mids = Vec::new();
    for dir in [Direction::Climb, Direction::Conventional] {
        let kind = OpKind::Contour {
            side: Side::Outside,
            direction: dir,
        };
        let (pr, _, op) = setup(r_rect(), kind, 6.0, manual(1, &[0.5]));
        let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
        assert_eq!(lifts.len(), 1);
        assert!(approx(lifts[0].mid().distance(p(100.0, 50.0)), 3.0));
        mids.push(lifts[0].mid());
    }
    eprintln!("corner tab lift midpoints climb/conventional: {mids:?}");
}

// ---------------------------------------------------------------- shapes

#[test]
fn circle_only_arcs_lifts_are_arcs() {
    let (pr, _, op) = setup(circle(30.0), outside(), 6.0, count(3));
    let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
    assert_eq!(lifts.len(), 3);
    for l in &lifts {
        assert!(!l.all_linear);
        assert!(l.path.iter().all(|s| matches!(s, Segment::Arc { .. })));
        assert!(approx(l.len(), 11.0));
    }
}

#[test]
fn rounded_rect_tabs_avoid_arcs() {
    let c = rounded();
    let l = c.length();
    let (pos, note) = tab_positions(&c, &count(4), 1, 6.0);
    assert_eq!((pos.len(), note), (4, None));
    let m = 2.5 + 6.0;
    for t in &pos {
        let s = t * l;
        let mut acc = 0.0;
        let mut found = false;
        for seg in &c.segments {
            let len = seg.length();
            found |=
                matches!(seg, Segment::Line { .. }) && s - acc >= m - T && acc + len - s >= m - T;
            acc += len;
        }
        assert!(found, "tab at s {s} not in a line zone");
    }
    for side in [Side::Outside, Side::Inside] {
        let kind = OpKind::Contour {
            side,
            direction: Direction::Climb,
        };
        let (pr, _, op) = setup(c.clone(), kind, 6.0, count(4));
        let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
        assert_eq!(lifts.len(), 4);
        for lf in &lifts {
            assert!(lf.all_linear && approx(lf.len(), 11.0), "{side:?} {lf:?}");
        }
    }
}

#[test]
fn inside_and_outside_bridge_on_same_part_edge() {
    let r = r_rect();
    let mut per_side = Vec::new();
    for kind in [outside(), inside(), on()] {
        let (pr, _, op) = setup(r.clone(), kind, 6.0, count(4));
        let lifts = check_tabbed(&pr, &run(&pr, op).unwrap());
        let mut s: Vec<f64> = lifts.iter().map(|l| r.closest(l.mid()).0).collect();
        s.sort_by(f64::total_cmp);
        // bridge = lifted length − D on straight edges
        for l in &lifts {
            assert!(approx(l.len() - 6.0, 5.0));
        }
        per_side.push(s);
    }
    for s in &per_side[1..] {
        for (a, b) in s.iter().zip(&per_side[0]) {
            assert!(approx(*a, *b), "{per_side:?}");
        }
    }
    assert_eq!(per_side[0].len(), 4);
}

#[test]
fn two_inside_loops_each_get_their_tab() {
    // dumbbell: two 30×30 squares joined by a 4 mm neck; Inside Ø6 splits into two loops
    let c = poly(
        &[
            (0.0, 0.0, 0.0),
            (30.0, 0.0, 0.0),
            (30.0, 13.0, 0.0),
            (50.0, 13.0, 0.0),
            (50.0, 0.0, 0.0),
            (80.0, 0.0, 0.0),
            (80.0, 30.0, 0.0),
            (50.0, 30.0, 0.0),
            (50.0, 17.0, 0.0),
            (30.0, 17.0, 0.0),
            (30.0, 30.0, 0.0),
            (0.0, 30.0, 0.0),
        ],
        true,
    );
    let l = c.length();
    // tabs at item s 15 = (15,0) and s 91 = (65,0)
    let (pr, _, op) = setup(c, inside(), 6.0, manual(1, &[15.0 / l, 91.0 / l]));
    let tp = run(&pr, op).unwrap();
    let lifts = check_tabbed(&pr, &tp);
    assert_eq!(lifts.len(), 2);
    assert!(lifts.iter().any(|l| l.mid().approx_eq(p(15.0, 3.0), T)));
    assert!(
        lifts.iter().any(|l| l.mid().approx_eq(p(65.0, 3.0), T)),
        "{:?}",
        lifts.iter().map(Lift::mid).collect::<Vec<_>>()
    );
}

#[test]
fn open_contour_on_and_closed_on() {
    let open = poly(
        &[(0.0, 0.0, 0.0), (50.0, 0.0, 0.0), (50.0, 30.0, 0.0)],
        false,
    );
    let (pr, id, op) = setup(open, on(), 6.0, manual(1, &[0.5]));
    let tp = run(&pr, op).unwrap();
    assert_eq!(tp.notes, [ToolpathNote::TabsOnOpenContour(id)]);
    assert!(check_tabbed(&pr, &tp).is_empty());
    // markers: tab_positions does not know about open/closed (the caller filters); no panic
    let _ = tab_positions(
        &poly(&[(0.0, 0.0, 0.0), (50.0, 0.0, 0.0)], false),
        &count(2),
        1,
        6.0,
    );
}

#[test]
fn degenerate_contour_tab_positions() {
    // zero-length closed contour: no tabs, no panic
    let c = Contour {
        segments: vec![Segment::Line {
            start: p(1.0, 1.0),
            end: p(1.0, 1.0),
        }],
        closed: true,
    };
    let (pos, _) = tab_positions(&c, &count(4), 1, 6.0);
    assert!(pos.is_empty());
}
