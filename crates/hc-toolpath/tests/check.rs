//! M8 acceptance tests (specs/M8.md): test 0 (clean fixture), one + and one − per rule, 29 (order).

use hc_geom::{Circle, Contour, Point};
use hc_project::{
    Direction, Geom, Item, ManualTabs, OpKind, Operation, Project, Side, Source, TabPlacement,
    Tool, ToolKind,
};
use hc_toolpath::{check, generate, Issue, RuleId, Severity};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Contour {
    let v = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    Contour::from_bulge_vertices(&v.map(|(x, y)| (p(x, y), 0.0)), true)
}

fn circle(x: f64, y: f64, d: f64) -> Contour {
    Circle {
        center: p(x, y),
        radius: d / 2.0,
    }
    .to_contour()
}

fn add(pr: &mut Project, geom: Geom) -> u64 {
    let id = pr.next_id;
    pr.next_id += 1;
    pr.items.push(Item {
        id,
        layer: "0".into(),
        source: Source::Drawn,
        geom,
    });
    id
}

fn t1() -> Tool {
    Tool {
        id: 1,
        name: "Flat".into(),
        kind: ToolKind::FlatEndMill,
        diameter: 3.175,
        flute_length: 12.0,
        flutes: 2,
        feed: 600.0,
        plunge_feed: 150.0,
        rpm: 16000,
        depth_per_pass: 2.0,
        stepover_pct: 45.0,
        comment: String::new(),
    }
}

fn t2() -> Tool {
    Tool {
        id: 2,
        name: "Drill".into(),
        kind: ToolKind::Drill,
        diameter: 5.0,
        flute_length: 20.0,
        flutes: 2,
        feed: 100.0,
        plunge_feed: 100.0,
        rpm: 3000,
        depth_per_pass: 2.0,
        stepover_pct: 45.0,
        comment: String::new(),
    }
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

struct Fx {
    pr: Project,
    s: u64,
    c: u64,
    c2: u64,
    contour: u64,
    pocket: u64,
    drill: u64,
}

impl Fx {
    fn op(&mut self, id: u64) -> &mut Operation {
        self.pr.operations.iter_mut().find(|o| o.id == id).unwrap()
    }

    fn issues(&self) -> Vec<Issue> {
        check(&self.pr, &generate(&self.pr))
    }

    fn rule(&self, r: RuleId) -> Vec<Issue> {
        self.issues().into_iter().filter(|i| i.rule == r).collect()
    }

    /// Exactly one issue of rule `r`; returned for further checks.
    fn one(&self, r: RuleId) -> Issue {
        let v = self.rule(r);
        assert_eq!(v.len(), 1, "{r:?}: {:#?}", self.issues());
        v[0].clone()
    }

    fn none(&self, r: RuleId) {
        assert!(self.rule(r).is_empty(), "{r:?}: {:#?}", self.issues());
    }

    /// Adds an op with `tool` on `items`; depth 10, dpp 2.
    fn add_op(&mut self, kind: OpKind, tool: &Tool, items: Vec<u64>) -> u64 {
        let id = self.pr.add_operation("x", kind, tool, items);
        self.op(id).depth = 10.0;
        id
    }

    /// Only the ops in `keep` stay enabled.
    fn only(&mut self, keep: &[u64]) {
        for o in &mut self.pr.operations {
            o.enabled = keep.contains(&o.id);
        }
    }
}

fn ok() -> Fx {
    let mut pr = Project::new();
    let s = add(&mut pr, Geom::Contour(rect(10.0, 10.0, 40.0, 40.0)));
    let c = add(&mut pr, Geom::Contour(circle(70.0, 70.0, 5.0)));
    let c2 = add(&mut pr, Geom::Contour(circle(70.0, 30.0, 20.0)));
    let contour = pr.add_operation("Contour 1", outside(), &t1(), vec![s]);
    let pocket = pr.add_operation(
        "Pocket 1",
        OpKind::Pocket { stepover_pct: 0.0 },
        &t1(),
        vec![c2],
    );
    let drill = pr.add_operation("Drill 1", OpKind::Drill { peck: None }, &t2(), vec![c]);
    let mut fx = Fx {
        pr,
        s,
        c,
        c2,
        contour,
        pocket,
        drill,
    };
    let o = fx.op(contour);
    o.depth = 10.0;
    o.depth_per_pass = 2.0;
    o.tabs.enabled = true;
    o.tabs.placement = TabPlacement::Count { count: 4 };
    o.tabs.width = 5.0;
    o.tabs.height = 2.0;
    fx.op(pocket).depth = 3.0;
    fx.op(drill).depth = 10.0;
    fx
}

/// An open L-polyline in the free corner of the stock.
fn add_l(fx: &mut Fx) -> u64 {
    let l = Contour::from_bulge_vertices(
        &[
            (p(10.0, 60.0), 0.0),
            (p(10.0, 90.0), 0.0),
            (p(40.0, 90.0), 0.0),
        ],
        false,
    );
    add(&mut fx.pr, Geom::Contour(l))
}

#[test]
fn t0_clean_fixture() {
    let fx = ok();
    assert!(generate(&fx.pr).iter().all(|(_, r)| r.is_ok()));
    assert_eq!(fx.issues(), []);
}

#[test]
fn r1_missing_tool() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).tool = 9;
    let i = fx.one(RuleId::MissingTool);
    assert_eq!(
        (i.op, i.item, i.severity),
        (Some(id), None, Severity::Error)
    );
    assert_eq!(i.message, "Uses tool T9, which is not in the project");
    fx.op(id).enabled = false;
    fx.none(RuleId::MissingTool);
    assert_eq!(fx.issues(), []);
}

#[test]
fn r2_missing_item() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).items.push(999);
    let i = fx.one(RuleId::MissingItem);
    assert_eq!((i.op, i.item), (Some(id), Some(999)));
    // the ToolpathError::MissingItem is not reported a second time
    assert_eq!(fx.issues().len(), 1);
    ok().none(RuleId::MissingItem);
}

#[test]
fn r3_duplicate_id() {
    let mut fx = ok();
    let mut dup = fx.pr.items[0].clone();
    dup.id = fx.s;
    fx.pr.items.push(dup);
    let i = fx.one(RuleId::DuplicateId);
    assert_eq!((i.op, i.item), (None, None));
    ok().none(RuleId::DuplicateId);
}

#[test]
fn r4_duplicate_tool_id() {
    let mut fx = ok();
    fx.pr.tools.push(t1());
    fx.one(RuleId::DuplicateToolId);
    ok().none(RuleId::DuplicateToolId);
}

#[test]
fn r5_id_counter() {
    let mut fx = ok();
    fx.pr.next_id = fx.drill; // the largest id
    let i = fx.one(RuleId::IdCounter);
    assert_eq!(i.severity, Severity::Warning);
    assert_eq!(fx.issues().len(), 1);
    ok().none(RuleId::IdCounter);
}

#[test]
fn r6_invalid_machine() {
    let mut fx = ok();
    fx.pr.machine.safe_z = 1.0;
    fx.one(RuleId::InvalidMachine);
    assert_eq!(fx.issues().len(), 1, "not once per op");
    let mut fx = ok();
    fx.pr.machine.tool_change_z = f64::NAN;
    let i = fx.one(RuleId::InvalidMachine);
    assert_eq!(i.message, "Tool change Z must be at or above safe Z");
    let mut fx = ok();
    fx.pr.machine.tool_change_z = fx.pr.machine.safe_z;
    fx.none(RuleId::InvalidMachine);
}

#[test]
fn r7_invalid_stock() {
    let mut fx = ok();
    fx.pr.stock.thickness = 0.0;
    let i = fx.one(RuleId::InvalidStock);
    assert_eq!(i.message, "Stock thickness must be greater than 0");
    ok().none(RuleId::InvalidStock);
}

#[test]
fn r8_invalid_value() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).depth_per_pass = 0.0;
    let i = fx.one(RuleId::InvalidValue);
    assert_eq!(i.message, "Invalid value: depth_per_pass");
    assert_eq!(i.op, Some(id));
    ok().none(RuleId::InvalidValue);
}

#[test]
fn r9_no_items() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).items.clear();
    fx.one(RuleId::NoItems);
    ok().none(RuleId::NoItems);
}

#[test]
fn r10_open_contour() {
    let mut fx = ok();
    let l = add_l(&mut fx);
    let id = fx.pocket;
    fx.op(id).items = vec![l];
    assert_eq!(fx.one(RuleId::OpenContour).item, Some(l));
    let mut fx = ok();
    let l = add_l(&mut fx);
    let e = fx.add_op(OpKind::Engrave, &t1(), vec![l]);
    fx.op(e).depth = 1.0;
    fx.none(RuleId::OpenContour);
}

#[test]
fn r11_unsupported_item() {
    let mut fx = ok();
    let (id, s) = (fx.drill, fx.s);
    fx.op(id).items = vec![s];
    assert_eq!(fx.one(RuleId::UnsupportedItem).item, Some(s));
    ok().none(RuleId::UnsupportedItem);
}

#[test]
fn r12_tool_too_large() {
    let mut fx = ok();
    let mut big = t1();
    big.id = 3;
    big.diameter = 6.0;
    let c = fx.c;
    let id = fx.add_op(inside(), &big, vec![c]);
    fx.op(id).depth = 2.0;
    let i = fx.one(RuleId::ToolTooLarge);
    assert_eq!((i.op, i.item), (Some(id), Some(c)));
    assert_eq!(
        i.message,
        "Tool too large for the circle Ø5.000 mm on layer 0"
    );
    let mut fx = ok();
    let id = fx.add_op(inside(), &t1(), vec![c]);
    fx.op(id).depth = 2.0;
    fx.none(RuleId::ToolTooLarge);
}

#[test]
fn r13_drill_bit_side_cut() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).tool = 2;
    assert_eq!(
        fx.one(RuleId::DrillBitSideCut).message,
        "T2 is a drill bit and cannot mill"
    );
    ok().none(RuleId::DrillBitSideCut);
}

#[test]
fn r14_depth_beyond_flutes() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).depth = 12.5;
    assert_eq!(
        fx.one(RuleId::DepthBeyondFlutes).message,
        "Depth 12.500 mm is deeper than the flute length 12.000 mm of T1"
    );
    fx.op(id).depth = 12.0;
    fx.none(RuleId::DepthBeyondFlutes);
    // bad tool data fires: flute length 0 or NaN
    fx.pr.tools[0].flute_length = f64::NAN;
    assert_eq!(fx.rule(RuleId::DepthBeyondFlutes).len(), 2, "both T1 ops");
}

#[test]
fn r15_spindle_off() {
    let mut fx = ok();
    let id = fx.contour;
    fx.op(id).rpm = 0;
    let i = fx.one(RuleId::SpindleOff);
    assert_eq!(
        (i.severity, i.message.as_str()),
        (Severity::Warning, "Spindle speed is 0 (S0)")
    );
    fx.op(id).rpm = 1;
    fx.none(RuleId::SpindleOff);
}

#[test]
fn r16_feed_vs_tool() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).feed = 600.0 * 25.4;
    assert_eq!(
        fx.one(RuleId::FeedVsTool).message,
        "Feed 15240.000 mm/min is far from the tool's 600.000 mm/min"
    );
    let mut fx = ok();
    fx.op(id).plunge_feed = 150.0 * 25.4;
    assert!(fx
        .one(RuleId::FeedVsTool)
        .message
        .starts_with("Plunge feed 3810.000"));
    fx.op(id).feed = 600.0 * 25.4;
    assert_eq!(fx.rule(RuleId::FeedVsTool).len(), 2, "one issue per field");
    // review ruling 4c: factor 10, so 1/5 (aluminium) and 9× stay silent
    let mut fx = ok();
    fx.op(id).plunge_feed = 150.0 / 5.0;
    fx.none(RuleId::FeedVsTool);
    fx.op(id).feed = 600.0 * 9.0;
    fx.none(RuleId::FeedVsTool);
    // 4g: feed 0 is InvalidValue only
    fx.op(id).feed = 0.0;
    fx.none(RuleId::FeedVsTool);
    fx.one(RuleId::InvalidValue);
}

#[test]
fn r17_stepover_high() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).kind = OpKind::Pocket { stepover_pct: 60.0 };
    assert_eq!(
        fx.one(RuleId::StepoverHigh).message,
        "Stepover 60 % may leave material between rings"
    );
    fx.op(id).kind = OpKind::Pocket { stepover_pct: 50.0 };
    fx.none(RuleId::StepoverHigh);
}

#[test]
fn r18_through_cut() {
    let mut fx = ok();
    let id = fx.pocket;
    fx.op(id).depth = 11.01;
    let i = fx.one(RuleId::ThroughCut);
    assert_eq!(
        i.message,
        "Cuts 1.010 mm into the spoilboard (depth 11.010 mm, stock 10.000 mm)"
    );
    fx.op(id).depth = 11.0;
    fx.none(RuleId::ThroughCut);
}

#[test]
fn r19_tabs_not_through() {
    let mut fx = ok();
    let id = fx.contour;
    fx.op(id).depth = 5.0;
    fx.one(RuleId::TabsNotThrough);
    ok().none(RuleId::TabsNotThrough);
}

#[test]
fn r20_tab_too_high() {
    let mut fx = ok();
    let id = fx.contour;
    fx.op(id).tabs.height = 10.0;
    assert_eq!(
        fx.one(RuleId::TabTooHigh).message,
        "Tab height 10.000 mm is not below the depth 10.000 mm"
    );
    fx.op(id).tabs.height = 9.99;
    fx.none(RuleId::TabTooHigh);
}

#[test]
fn r21_tabs_reduced() {
    let mut fx = ok();
    let (id, s) = (fx.contour, fx.s);
    fx.op(id).tabs.placement = TabPlacement::Count { count: 20 };
    let i = fx.one(RuleId::TabsReduced);
    assert_eq!(i.item, Some(s));
    assert!(i.message.contains("of 20 tabs fit"), "{}", i.message);
    ok().none(RuleId::TabsReduced);
}

#[test]
fn r22_tabs_on_open_contour() {
    let mut fx = ok();
    let l = add_l(&mut fx);
    let on = OpKind::Contour {
        side: Side::On,
        direction: Direction::Climb,
    };
    let id = fx.add_op(on, &t1(), vec![l]);
    fx.op(id).depth = 5.0;
    fx.op(id).tabs.enabled = true;
    assert_eq!(fx.one(RuleId::TabsOnOpenContour).item, Some(l));
    fx.op(id).tabs.enabled = false;
    fx.none(RuleId::TabsOnOpenContour);
}

#[test]
fn r23_no_tabs_through_cut() {
    let mut fx = ok();
    let (id, s) = (fx.contour, fx.s);
    fx.op(id).tabs.enabled = false;
    let i = fx.one(RuleId::NoTabsThroughCut);
    assert_eq!((i.op, i.item), (Some(id), Some(s)));
    assert_eq!(
        i.message,
        "The part may come loose: through-cut without tabs"
    );
    // closed On contour, Count 0 (no positions) counts too
    fx.op(id).kind = OpKind::Contour {
        side: Side::On,
        direction: Direction::Climb,
    };
    fx.op(id).tabs.enabled = true;
    fx.op(id).tabs.placement = TabPlacement::Count { count: 0 };
    fx.one(RuleId::NoTabsThroughCut);
    // amendment A1: an Inside through-cut without tabs is not reported
    let mut fx = ok();
    let c2 = fx.c2;
    let ins = fx.add_op(inside(), &t1(), vec![c2]);
    fx.only(&[ins]);
    fx.none(RuleId::NoTabsThroughCut);
    // not through
    let mut fx = ok();
    fx.op(id).tabs.enabled = false;
    fx.op(id).depth = 5.0;
    fx.none(RuleId::NoTabsThroughCut);
}

#[test]
fn r24_tab_on_corner() {
    let mut fx = ok();
    let (id, s) = (fx.contour, fx.s);
    fx.op(id).tabs.manual = vec![ManualTabs {
        item: s,
        positions: vec![0.0],
    }];
    let i = fx.one(RuleId::TabOnCorner);
    assert_eq!(i.item, Some(s));
    assert_eq!(
        i.message,
        "Tab 1 sits on a corner and may be weak or misplaced"
    );
    fx.op(id).tabs.manual[0].positions = vec![0.125];
    fx.none(RuleId::TabOnCorner);
    // arcs only: no corner
    let mut fx = ok();
    let c2 = fx.c2;
    let o = fx.op(id);
    o.items = vec![c2];
    fx.none(RuleId::TabOnCorner);
    assert!(fx.issues().is_empty(), "{:#?}", fx.issues());
}

#[test]
fn r25_drill_size() {
    let mut fx = ok();
    let id = fx.drill;
    fx.pr.tools[1].diameter = 5.1;
    let i = fx.one(RuleId::DrillSize);
    assert_eq!(i.message, "Hole Ø5.000 mm, drill Ø5.100 mm");
    assert_eq!(i.item, Some(fx.c));
    fx.pr.tools[1].diameter = 5.04;
    fx.none(RuleId::DrillSize);
    // review ruling 4a: a smaller bit (pilot, tap) is fine
    fx.pr.tools[1].diameter = 4.2;
    fx.none(RuleId::DrillSize);
    fx.pr.tools[1].diameter = 5.1;
    let pt = add(&mut fx.pr, Geom::Point(p(80.0, 80.0)));
    fx.op(id).items = vec![pt];
    fx.none(RuleId::DrillSize);
}

#[test]
fn r26_shared_edge() {
    let mut fx = ok();
    let s = fx.s;
    let inner = add(&mut fx.pr, Geom::Contour(rect(10.0, 10.0, 25.0, 25.0)));
    let id = fx.pocket;
    fx.op(id).items = vec![s, inner];
    let i = fx.one(RuleId::SharedEdge);
    // on the inner item (the island)
    assert_eq!((i.op, i.item), (Some(id), Some(inner)));
    assert_eq!(
        i.message,
        "The closed contour on layer 0 touches the closed contour on layer 0: the pocket treats them as one wall"
    );
    let mut fx = ok();
    let inner = add(&mut fx.pr, Geom::Contour(rect(15.0, 15.0, 25.0, 25.0)));
    fx.op(id).items = vec![s, inner];
    fx.none(RuleId::SharedEdge);
    // review ruling 4f: an L of two touching rectangles is not nested
    let mut fx = ok();
    let a = add(&mut fx.pr, Geom::Contour(rect(10.0, 10.0, 25.0, 40.0)));
    let b = add(&mut fx.pr, Geom::Contour(rect(25.0, 10.0, 40.0, 25.0)));
    fx.op(id).items = vec![a, b];
    fx.none(RuleId::SharedEdge);
}

#[test]
fn r27_outside_stock() {
    let mut fx = ok();
    let id = fx.contour;
    fx.pr.stock.min_x = 20.0;
    fx.pr.stock.width = 80.0;
    let i = fx.one(RuleId::OutsideStock);
    assert_eq!(i.op, Some(id));
    // tool centre at 10 - r, 20 - 10 + r from the stock edge; the cutter reaches r further
    assert_eq!(i.message, "Cuts up to 13.175 mm outside the stock");
    let mut fx = ok();
    fx.pr.stock.min_x = 10.0;
    fx.pr.stock.min_y = 10.0;
    fx.pr.stock.width = 30.0;
    fx.pr.stock.height = 30.0;
    fx.only(&[id]);
    fx.none(RuleId::OutsideStock);
}

#[test]
fn t29_order_and_determinism() {
    let mut fx = ok();
    let (c, pk) = (fx.contour, fx.pocket);
    fx.op(c).tool = 9; // 1 on op 1
    fx.op(pk).depth = 12.5; // 14 on op 2
    fx.op(pk).rpm = 0; // 15 on op 2
    fx.pr.tools.push(t1()); // project level: DuplicateToolId
    let a = fx.issues();
    assert_eq!(a, fx.issues());
    let rules: Vec<(RuleId, Option<u64>)> = a.iter().map(|i| (i.rule, i.op)).collect();
    assert_eq!(
        rules,
        [
            (RuleId::DuplicateToolId, None),
            (RuleId::MissingTool, Some(c)),
            (RuleId::DepthBeyondFlutes, Some(pk)),
            (RuleId::SpindleOff, Some(pk)),
            (RuleId::ThroughCut, Some(pk)), // 12.5 mm is also 2.5 mm into the spoilboard
        ]
    );
}

// ---------------------------------------------------------------- M10 test 8: origin independence

#[test]
fn m10_08_checks_ignore_the_work_origin() {
    use hc_project::WorkOrigin;
    let origins = [
        WorkOrigin::DrawingOrigin,
        WorkOrigin::StockBottomLeft,
        WorkOrigin::StockCenter,
        WorkOrigin::Point(p(500.0, 500.0)),
    ];
    let mut fx = ok();
    fx.pr.stock.origin = WorkOrigin::Point(p(500.0, 500.0));
    assert_eq!(fx.issues(), []);
    for o in origins {
        let mut fx = ok();
        fx.pr.stock.origin = o;
        fx.pr.stock.min_x = 20.0;
        fx.pr.stock.width = 80.0;
        fx.one(RuleId::OutsideStock);
    }
}

// ---------------------------------------------------------------- M10 test 15: shared-edge island (M5 ruling e)

fn shared_edge_pocket() -> (Project, u64) {
    use hc_geom::shapes;
    let mut pr = Project::new();
    let ids = pr.add_drawn(vec![
        Geom::Contour(shapes::rect(p(0.0, 0.0), 40.0, 40.0, 0.0).unwrap()),
        Geom::Contour(shapes::rect(p(0.0, 10.0), 15.0, 20.0, 0.0).unwrap()),
    ]);
    let id = pr.add_operation("Pocket", OpKind::Pocket { stepover_pct: 0.0 }, &t1(), ids);
    (pr, id)
}

/// Cutting-feed tool centres (below Z 0), sampled every 0.05 mm along lines and arcs.
fn feed_samples(tp: &hc_toolpath::Toolpath) -> Vec<Point> {
    use hc_toolpath::Move;
    let mut cur = p(0.0, 0.0);
    let mut out = Vec::new();
    for m in &tp.moves {
        match *m {
            Move::Rapid(to) => cur = to.xy(),
            Move::Linear { to, .. } => {
                let n = (cur.distance(to.xy()) / 0.05).ceil().max(1.0) as usize;
                if to.z < -1e-6 {
                    out.extend((0..=n).map(|k| cur + (to.xy() - cur) * (k as f64 / n as f64)));
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
                    out.extend((0..=n).map(|k| {
                        let a = a0 + (a1 - a0) * k as f64 / n as f64;
                        center + p(a.cos(), a.sin()) * rad
                    }));
                }
                cur = to.xy();
            }
            Move::Drill { .. } => {}
        }
    }
    out
}

/// Signed depth of `q` inside the axis-aligned rect (> 0 inside, < 0 = minus the distance to it).
fn rect_depth(q: Point, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    let (dx, dy) = ((q.x - x0).min(x1 - q.x), (q.y - y0).min(y1 - q.y));
    if dx >= 0.0 && dy >= 0.0 {
        return dx.min(dy);
    }
    let ox = (x0 - q.x).max(q.x - x1).max(0.0);
    let oy = (y0 - q.y).max(q.y - y1).max(0.0);
    -(ox * ox + oy * oy).sqrt()
}

#[test]
fn m10_15_pocket_island_sharing_an_edge() {
    use hc_toolpath::generate_op;
    let (pr, id) = shared_edge_pocket();
    let op = pr.operations.iter().find(|o| o.id == id).unwrap().clone();
    let tp = generate_op(&pr, &op).expect("pocket with a shared-edge island");
    let r = t1().diameter / 2.0;
    let pts = feed_samples(&tp);
    assert!(!pts.is_empty());
    for q in pts {
        // tool edge into the island or past the boundary wall
        let island = rect_depth(q, 0.0, 10.0, 15.0, 30.0) + r;
        let wall = -rect_depth(q, 0.0, 0.0, 40.0, 40.0) + r;
        assert!(
            island.max(wall) <= 1e-3,
            "gouge {} at {q:?}",
            island.max(wall)
        );
    }
}

/// Architect review fix 1: two islands sharing an edge with each other inside the boundary.
#[test]
fn m10_15_two_islands_sharing_an_edge() {
    use hc_geom::shapes;
    use hc_toolpath::generate_op;
    let mut pr = Project::new();
    let ids = pr.add_drawn(vec![
        Geom::Contour(shapes::rect(p(0.0, 0.0), 40.0, 40.0, 0.0).unwrap()),
        Geom::Contour(shapes::rect(p(10.0, 10.0), 10.0, 20.0, 0.0).unwrap()),
        Geom::Contour(shapes::rect(p(20.0, 10.0), 10.0, 20.0, 0.0).unwrap()),
    ]);
    let id = pr.add_operation("Pocket", OpKind::Pocket { stepover_pct: 0.0 }, &t1(), ids);
    let op = pr.operations.iter().find(|o| o.id == id).unwrap().clone();
    let tp = generate_op(&pr, &op).expect("pocket with two touching islands");
    let r = t1().diameter / 2.0;
    let pts = feed_samples(&tp);
    assert!(!pts.is_empty());
    for q in pts {
        let a = rect_depth(q, 10.0, 10.0, 20.0, 30.0) + r;
        let b = rect_depth(q, 20.0, 10.0, 30.0, 30.0) + r;
        let wall = -rect_depth(q, 0.0, 0.0, 40.0, 40.0) + r;
        let g = a.max(b).max(wall);
        assert!(g <= 1e-3, "gouge {g} at {q:?}");
    }
}

#[test]
fn m10_15_shared_edge_island_is_warned() {
    let (pr, _) = shared_edge_pocket();
    let issues = check(&pr, &generate(&pr));
    let shared: Vec<_> = issues
        .iter()
        .filter(|i| i.rule == RuleId::SharedEdge)
        .collect();
    assert_eq!(shared.len(), 1, "{issues:#?}");
}
