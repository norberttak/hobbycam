//! M8 edge cases (tester): threshold boundaries, inch display units, duplicates, disabled ops,
//! ordering, tab rules, performance. Fixture = `ok()` of tests/check.rs (spec "Acceptance tests").

use hc_geom::{BBox, Circle, Contour, Point};
use hc_project::{
    Direction, Geom, Item, ManualTabs, OpKind, Operation, Project, Side, Source, TabPlacement,
    Tool, ToolKind, Units,
};
use hc_toolpath::{check, generate, Issue, RuleId, Severity};
use std::collections::HashSet;
use std::time::{Duration, Instant};

const BRACKET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dxf/autocad2018_bracket_inch.dxf"
);

// ---------------------------------------------------------------- fixture

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn poly(v: &[(f64, f64)], closed: bool) -> Contour {
    let v: Vec<_> = v.iter().map(|&(x, y)| (p(x, y), 0.0)).collect();
    Contour::from_bulge_vertices(&v, closed)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Contour {
    poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], true)
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
        feed: 100.0,
        plunge_feed: 100.0,
        rpm: 3000,
        ..t1()
    }
}

fn contour_kind(side: Side) -> OpKind {
    OpKind::Contour {
        side,
        direction: Direction::Climb,
    }
}

/// The spec's severity column (rules table), independent of the implementation.
fn spec_severity(r: RuleId) -> Severity {
    use RuleId::*;
    match r {
        IdCounter | SpindleOff | FeedVsTool | StepoverHigh | ThroughCut | TabsNotThrough
        | TabTooHigh | TabsReduced | TabsOnOpenContour | NoTabsThroughCut | TabOnCorner
        | DrillSize | SharedEdge | OutsideStock => Severity::Warning,
        _ => Severity::Error,
    }
}

/// Every issue has the spec severity, and no (rule, op, item, message) appears twice.
fn sane(v: &[Issue]) {
    let mut seen = HashSet::new();
    for i in v {
        assert_eq!(i.severity, spec_severity(i.rule), "{i:?}");
        assert!(
            seen.insert((format!("{:?}", i.rule), i.op, i.item, i.message.clone())),
            "duplicate issue {i:?} in {v:#?}"
        );
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
        let v = check(&self.pr, &generate(&self.pr));
        sane(&v);
        v
    }

    fn rule(&self, r: RuleId) -> Vec<Issue> {
        self.issues().into_iter().filter(|i| i.rule == r).collect()
    }

    fn count(&self, r: RuleId) -> usize {
        self.rule(r).len()
    }

    fn one(&self, r: RuleId) -> Issue {
        let v = self.rule(r);
        assert_eq!(v.len(), 1, "{r:?}: {:#?}", self.issues());
        v[0].clone()
    }

    fn add_op(&mut self, kind: OpKind, tool: &Tool, items: Vec<u64>) -> u64 {
        let id = self.pr.add_operation("x", kind, tool, items);
        self.op(id).depth = 10.0;
        id
    }

    fn only(&mut self, keep: &[u64]) {
        for o in &mut self.pr.operations {
            o.enabled = keep.contains(&o.id);
        }
    }

    fn tabs_at(&mut self, op: u64, item: u64, positions: Vec<f64>) {
        self.op(op).tabs.manual = vec![ManualTabs { item, positions }];
    }
}

fn ok() -> Fx {
    let mut pr = Project::new();
    let s = add(&mut pr, Geom::Contour(rect(10.0, 10.0, 40.0, 40.0)));
    let c = add(&mut pr, Geom::Contour(circle(70.0, 70.0, 5.0)));
    let c2 = add(&mut pr, Geom::Contour(circle(70.0, 30.0, 20.0)));
    let contour = pr.add_operation("Contour 1", contour_kind(Side::Outside), &t1(), vec![s]);
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

fn inch() -> Fx {
    let mut fx = ok();
    fx.pr.units = Units::Inch;
    fx
}

// ---------------------------------------------------------------- fixture sanity

#[test]
fn fixture_uses_spec_values() {
    let mut fx = ok();
    // add_operation copies the tool's 45 % stepover over the 0 passed in
    let pk = fx.pocket;
    assert_eq!(fx.op(pk).kind, OpKind::Pocket { stepover_pct: 45.0 });
    assert_eq!(fx.issues(), []);
    assert_eq!(inch().issues(), [], "inch display units change nothing");
}

// ---------------------------------------------------------------- thresholds (mm and inch)

#[test]
fn feed_factor_boundaries() {
    for units in [Units::Mm, Units::Inch] {
        let n = |feed: f64, plunge: f64| {
            let mut fx = ok();
            fx.pr.units = units;
            let pk = fx.pocket;
            fx.op(pk).feed = feed;
            fx.op(pk).plunge_feed = plunge;
            fx.count(RuleId::FeedVsTool)
        };
        // review ruling 4c: factor 10 (was 4)
        assert_eq!(n(6000.0, 150.0), 0, "exactly 10x");
        assert_eq!(n(6000.0 + 1e-6, 150.0), 1);
        assert_eq!(n(60.0, 150.0), 0, "exactly 1/10");
        assert_eq!(n(60.0 - 1e-6, 150.0), 1);
        assert_eq!(n(600.0, 1500.0), 0, "plunge exactly 10x");
        assert_eq!(n(600.0, 1500.0 + 1e-6), 1);
        assert_eq!(n(600.0, 15.0), 0, "plunge exactly 1/10");
        assert_eq!(n(600.0, 15.0 - 1e-6), 1);
        assert_eq!(n(600.0 / 25.4, 150.0 * 25.4), 2, "one per field");
    }
}

#[test]
fn feed_rule_drill_and_zero_tool_values() {
    let mut fx = ok();
    let d = fx.drill;
    // Drill: `feed` is not checked, plunge is
    fx.op(d).feed = 100.0 * 25.4;
    assert_eq!(fx.count(RuleId::FeedVsTool), 0);
    fx.op(d).plunge_feed = 100.0 * 25.4;
    let i = fx.one(RuleId::FeedVsTool);
    assert_eq!((i.op, i.item), (Some(d), None));
    assert_eq!(
        i.message,
        "Plunge feed 2540.000 mm/min is far from the tool's 100.000 mm/min"
    );
    // tool value 0: rule silent
    let mut fx = ok();
    fx.pr.tools[0].feed = 0.0;
    fx.pr.tools[0].plunge_feed = 0.0;
    assert_eq!(fx.count(RuleId::FeedVsTool), 0);
}

#[test]
fn feed_message_in_inch() {
    let mut fx = inch();
    let pk = fx.pocket;
    fx.op(pk).feed = 600.0 * 25.4;
    let i = fx.one(RuleId::FeedVsTool);
    assert_eq!(
        i.message,
        "Feed 600.0000 in/min is far from the tool's 23.6220 in/min"
    );
}

#[test]
fn stepover_boundaries() {
    let n = |v: f64| {
        let mut fx = ok();
        let pk = fx.pocket;
        fx.op(pk).kind = OpKind::Pocket { stepover_pct: v };
        fx.count(RuleId::StepoverHigh)
    };
    assert_eq!(n(50.0), 0);
    assert_eq!(n(50.0 + 1e-10), 0, "within 1e-9");
    assert_eq!(n(50.001), 1);
    assert_eq!(n(100.0), 1);
    let mut fx = ok();
    fx.pr.tools[0].stepover_pct = 90.0;
    assert_eq!(
        fx.count(RuleId::StepoverHigh),
        0,
        "only the op value counts"
    );
}

#[test]
fn through_cut_boundaries() {
    for units in [Units::Mm, Units::Inch] {
        let n = |depth: f64| {
            let mut fx = ok();
            fx.pr.units = units;
            let pk = fx.pocket;
            fx.op(pk).depth = depth;
            fx.count(RuleId::ThroughCut)
        };
        assert_eq!(n(11.0), 0, "exactly 1 mm");
        assert_eq!(n(11.0 + 1e-6), 1);
        assert_eq!(n(10.0), 0);
    }
    let mut fx = inch();
    let pk = fx.pocket;
    fx.op(pk).depth = 11.01;
    assert_eq!(
        fx.one(RuleId::ThroughCut).message,
        "Cuts 0.0398 in into the spoilboard (depth 0.4335 in, stock 0.3937 in)"
    );
    // a drill (long flutes) also gets it, on its own op
    let mut fx = ok();
    let d = fx.drill;
    fx.op(d).depth = 15.0;
    assert_eq!(fx.one(RuleId::ThroughCut).op, Some(d));
}

#[test]
fn depth_beyond_flutes_boundaries_and_inch() {
    let n = |depth: f64| {
        let mut fx = ok();
        let pk = fx.pocket;
        fx.op(pk).depth = depth;
        fx.count(RuleId::DepthBeyondFlutes)
    };
    assert_eq!(n(12.0 + 0.5e-6), 0, "within TOL");
    assert_eq!(n(12.0 + 2e-6), 1);
    let mut fx = inch();
    let pk = fx.pocket;
    fx.op(pk).depth = 12.5;
    let i = fx.one(RuleId::DepthBeyondFlutes);
    assert_eq!(
        i.message,
        "Depth 0.4921 in is deeper than the flute length 0.4724 in of T1"
    );
    // flute length 0 (bad tool data) fires on both T1 ops
    let mut fx = ok();
    fx.pr.tools[0].flute_length = 0.0;
    assert_eq!(fx.count(RuleId::DepthBeyondFlutes), 2);
}

#[test]
fn tabs_not_through_boundaries() {
    let n = |depth: f64| {
        let mut fx = ok();
        let c = fx.contour;
        fx.op(c).depth = depth;
        fx.count(RuleId::TabsNotThrough)
    };
    assert_eq!(n(10.0 - 0.5e-6), 0, "within TOL");
    assert_eq!(n(10.0 - 2e-6), 1);
    assert_eq!(n(10.5), 0);
}

#[test]
fn tab_too_high_boundaries() {
    let n = |h: f64, depth: f64| {
        let mut fx = ok();
        let c = fx.contour;
        fx.op(c).depth = depth;
        fx.op(c).tabs.height = h;
        fx.count(RuleId::TabTooHigh)
    };
    assert_eq!(n(2.0, 2.0 + 2e-6), 0);
    assert_eq!(n(2.0, 2.0 + 0.5e-6), 1, "within TOL counts as equal");
    assert_eq!(n(3.0, 2.0), 1);
    let mut fx = inch();
    let c = fx.contour;
    fx.op(c).tabs.height = 10.0;
    assert_eq!(
        fx.one(RuleId::TabTooHigh).message,
        "Tab height 0.3937 in is not below the depth 0.3937 in"
    );
}

#[test]
fn drill_size_boundaries() {
    for units in [Units::Mm, Units::Inch] {
        let n = |d: f64| {
            let mut fx = ok();
            fx.pr.units = units;
            fx.pr.tools[1].diameter = d;
            fx.count(RuleId::DrillSize)
        };
        assert_eq!(n(5.05), 0, "exactly 0.05 larger");
        assert_eq!(n(4.95), 0, "exactly 0.05 smaller");
        assert_eq!(n(5.0501), 1);
        // review ruling 4a: only an oversize drill warns
        assert_eq!(n(4.9499), 0);
    }
    let mut fx = inch();
    fx.pr.tools[1].diameter = 5.1;
    assert_eq!(
        fx.one(RuleId::DrillSize).message,
        "Hole Ø0.1969 in, drill Ø0.2008 in"
    );
}

#[test]
fn drill_size_one_issue_per_hole_in_item_order() {
    let mut fx = ok();
    let d = fx.drill;
    let (c, c2) = (fx.c, fx.c2);
    let c3 = add(&mut fx.pr, Geom::Contour(circle(85.0, 85.0, 4.0)));
    fx.op(d).items = vec![c2, c3, c];
    let v = fx.rule(RuleId::DrillSize);
    // review ruling 4a: the Ø20 hole (undersize drill) is silent, the Ø4 one (oversize drill) warns
    assert_eq!(v.len(), 1, "only the Ø4 circle is smaller than the drill");
    assert_eq!(v[0].item, Some(c3));
    assert_eq!(v[0].message, "Hole Ø4.000 mm, drill Ø5.000 mm");
}

/// Closed polygon with a turn of `deg` at vertex B (arc length 30 from the start).
fn kinked(deg: f64) -> Contour {
    let (s, c) = deg.to_radians().sin_cos();
    let b = (50.0, 20.0);
    let cc = (b.0 + 20.0 * c, b.1 + 20.0 * s);
    poly(&[(20.0, 20.0), b, cc, (cc.0, 70.0), (20.0, 70.0)], true)
}

#[test]
fn corner_angle_boundary() {
    let n = |deg: f64| {
        let mut fx = ok();
        let k = add(&mut fx.pr, Geom::Contour(kinked(deg)));
        let c = fx.contour;
        fx.op(c).items = vec![k];
        let l = kinked(deg).length();
        fx.tabs_at(c, k, vec![30.0 / l]);
        fx.only(&[c]);
        fx.count(RuleId::TabOnCorner)
    };
    assert_eq!(n(29.0), 0);
    assert_eq!(n(31.0), 1);
    assert_eq!(n(10.0), 0);
    assert_eq!(n(60.0), 1);
}

#[test]
fn corner_tab_interval_edge_and_wrap() {
    let n = |s_mm: f64| {
        let mut fx = ok();
        let (c, s) = (fx.contour, fx.s);
        fx.tabs_at(c, s, vec![s_mm / 120.0]);
        fx.count(RuleId::TabOnCorner)
    };
    assert_eq!(n(2.5), 1, "corner exactly at the tab edge (w/2)");
    assert_eq!(n(2.6), 0);
    assert_eq!(n(30.0 + 2.4), 1, "second corner");
    assert_eq!(n(120.0 - 2.4), 1, "wraps to the start corner");
    assert_eq!(n(15.0), 0);
    // one issue per tab, numbered in position order
    let mut fx = ok();
    let (c, s) = (fx.contour, fx.s);
    fx.tabs_at(c, s, vec![0.125, 0.25, 0.375, 0.5]);
    let v = fx.rule(RuleId::TabOnCorner);
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v.iter().all(|i| i.item == Some(s) && i.op == Some(c)));
    let msgs: Vec<_> = v.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(
        msgs,
        [
            "Tab 2 sits on a corner and may be weak or misplaced",
            "Tab 4 sits on a corner and may be weak or misplaced"
        ]
    );
}

#[test]
fn rounded_rectangle_has_no_corner_tabs() {
    // 30x30 square with r=3 tangent fillets; tabs everywhere along it
    let k = (std::f64::consts::FRAC_PI_8).tan();
    let v = [
        (p(13.0, 10.0), 0.0),
        (p(37.0, 10.0), k),
        (p(40.0, 13.0), 0.0),
        (p(40.0, 37.0), k),
        (p(37.0, 40.0), 0.0),
        (p(13.0, 40.0), k),
        (p(10.0, 37.0), 0.0),
        (p(10.0, 13.0), k),
    ];
    let rr = Contour::from_bulge_vertices(&v, true);
    let mut fx = ok();
    let id = add(&mut fx.pr, Geom::Contour(rr));
    let c = fx.contour;
    fx.op(c).items = vec![id];
    fx.tabs_at(c, id, (0..16).map(|i| i as f64 / 16.0).collect());
    fx.only(&[c]);
    assert_eq!(fx.count(RuleId::TabOnCorner), 0, "{:#?}", fx.issues());
}

#[test]
fn outside_stock_boundaries() {
    // stock = S's bbox; the Outside path centre sits exactly r outside
    let n = |shrink_right: f64, shrink_left: f64, units: Units| {
        let mut fx = ok();
        fx.pr.units = units;
        let c = fx.contour;
        fx.only(&[c]);
        let st = &mut fx.pr.stock;
        st.min_x = 10.0 + shrink_left;
        st.min_y = 10.0;
        st.width = 30.0 - shrink_left - shrink_right;
        st.height = 30.0;
        fx.count(RuleId::OutsideStock)
    };
    for u in [Units::Mm, Units::Inch] {
        assert_eq!(n(0.0, 0.0, u), 0);
        assert_eq!(n(0.9e-3, 0.0, u), 0, "within the 1e-3 margin");
        assert_eq!(n(1.1e-3, 0.0, u), 1);
        assert_eq!(n(0.0, 0.9e-3, u), 0);
        assert_eq!(n(0.0, 1.1e-3, u), 1);
        assert_eq!(n(1.1e-3, 1.1e-3, u), 1, "one issue per op");
    }
}

#[test]
fn outside_stock_drill_and_message() {
    let mut fx = ok();
    let d = fx.drill;
    fx.pr.stock.width = 60.0; // hole C at x = 70
    let v = fx.rule(RuleId::OutsideStock);
    // the pocket at x 60..80 is cut too, the drill hole as well
    assert!(v.iter().any(|i| i.op == Some(d)), "{v:#?}");
    let i = v.iter().find(|i| i.op == Some(d)).unwrap();
    assert_eq!(i.item, None);
    // centre 10 beyond the edge, plus the drill radius 2.5
    assert_eq!(i.message, "Cuts up to 12.500 mm outside the stock");
}

#[test]
fn outside_stock_ignores_rapids_and_z0() {
    // Engrave at the stock edge: centre exactly on the edge
    let mut fx = ok();
    let e = fx.add_op(OpKind::Engrave, &t1(), vec![fx.s]);
    fx.op(e).depth = 1.0;
    fx.only(&[e]);
    fx.pr.stock.min_x = 10.0;
    fx.pr.stock.min_y = 10.0;
    fx.pr.stock.width = 30.0;
    fx.pr.stock.height = 30.0;
    assert_eq!(fx.count(RuleId::OutsideStock), 0);
}

// ---------------------------------------------------------------- tab rules

#[test]
fn tabs_reduced_message_and_once_per_item() {
    let mut fx = ok();
    let (c, s) = (fx.contour, fx.s);
    let s2 = add(&mut fx.pr, Geom::Contour(rect(10.0, 60.0, 40.0, 90.0)));
    fx.op(c).items = vec![s2, s];
    fx.op(c).tabs.placement = TabPlacement::Count { count: 40 };
    let v = fx.rule(RuleId::TabsReduced);
    assert_eq!(
        v.iter().map(|i| i.item).collect::<Vec<_>>(),
        [Some(s2), Some(s)],
        "item order of op.items"
    );
    assert!(v[0].message.starts_with("Only "));
    assert!(v[0]
        .message
        .ends_with(" of 40 tabs fit on the closed contour on layer 0"));
}

#[test]
fn tab_rules_need_tabs_enabled_and_a_contour() {
    // tabs disabled: no TabTooHigh / TabsNotThrough / TabOnCorner even if the values are bad
    let mut fx = ok();
    let (c, s) = (fx.contour, fx.s);
    fx.op(c).tabs.enabled = false;
    fx.op(c).tabs.height = 20.0;
    fx.op(c).depth = 5.0;
    fx.tabs_at(c, s, vec![0.0]);
    let v = fx.issues();
    assert_eq!(v, [], "{v:#?}");
    // Pocket / Drill with tabs.enabled (ignored data) get no tab rules
    let mut fx = ok();
    let (pk, d) = (fx.pocket, fx.drill);
    for id in [pk, d] {
        fx.op(id).tabs.enabled = true;
        fx.op(id).tabs.height = 50.0;
    }
    assert_eq!(fx.issues(), []);
}

#[test]
fn tab_too_high_and_not_through_together() {
    let mut fx = ok();
    let c = fx.contour;
    fx.op(c).depth = 2.0;
    fx.op(c).tabs.height = 3.0;
    let rules: Vec<_> = fx.issues().iter().map(|i| (i.rule, i.op)).collect();
    assert_eq!(
        rules,
        [
            (RuleId::TabsNotThrough, Some(c)),
            (RuleId::TabTooHigh, Some(c))
        ]
    );
}

#[test]
fn no_tabs_through_cut_sides() {
    // Outside, two items, tabs off: one per item, item order
    let mut fx = ok();
    let (c, s) = (fx.contour, fx.s);
    let s2 = add(&mut fx.pr, Geom::Contour(rect(10.0, 60.0, 40.0, 90.0)));
    fx.op(c).items = vec![s2, s];
    fx.op(c).tabs.enabled = false;
    let v = fx.rule(RuleId::NoTabsThroughCut);
    assert_eq!(
        v.iter().map(|i| i.item).collect::<Vec<_>>(),
        [Some(s2), Some(s)]
    );
    // tabs on only one item (manual positions empty for the other)
    let mut fx = ok();
    let s2 = add(&mut fx.pr, Geom::Contour(rect(10.0, 60.0, 40.0, 90.0)));
    fx.op(c).items = vec![s, s2];
    fx.tabs_at(c, s2, vec![]);
    assert_eq!(fx.one(RuleId::NoTabsThroughCut).item, Some(s2));
    // A1: Inside through-cut, tabs disabled or Count 0: silent
    for count in [None, Some(0)] {
        let mut fx = ok();
        let c2 = fx.c2;
        let ins = fx.add_op(contour_kind(Side::Inside), &t1(), vec![c2]);
        if let Some(n) = count {
            fx.op(ins).tabs.enabled = true;
            fx.op(ins).tabs.placement = TabPlacement::Count { count: n };
        }
        fx.only(&[ins]);
        assert_eq!(fx.count(RuleId::NoTabsThroughCut), 0);
    }
    // closed On contour through, tabs off: "part"
    let mut fx = ok();
    fx.op(c).kind = contour_kind(Side::On);
    fx.op(c).tabs.enabled = false;
    assert_eq!(
        fx.one(RuleId::NoTabsThroughCut).message,
        "The part may come loose: through-cut without tabs"
    );
    // exactly through within TOL counts as through
    let mut fx = ok();
    fx.op(c).tabs.enabled = false;
    fx.op(c).depth = 10.0 - 0.5e-6;
    assert_eq!(fx.count(RuleId::NoTabsThroughCut), 1);
    fx.op(c).depth = 10.0 - 2e-6;
    assert_eq!(fx.count(RuleId::NoTabsThroughCut), 0);
}

#[test]
fn open_on_contour_through_with_tabs_only_reports_the_note() {
    let mut fx = ok();
    let l = add(
        &mut fx.pr,
        Geom::Contour(poly(&[(10.0, 60.0), (10.0, 90.0), (40.0, 90.0)], false)),
    );
    let on = fx.add_op(contour_kind(Side::On), &t1(), vec![l]);
    fx.op(on).tabs.enabled = true;
    fx.op(on).tabs.placement = TabPlacement::Count { count: 4 };
    fx.op(on).tabs.width = 5.0;
    fx.op(on).tabs.height = 2.0;
    fx.only(&[on]);
    let v: Vec<_> = fx.issues().iter().map(|i| (i.rule, i.item)).collect();
    assert_eq!(v, [(RuleId::TabsOnOpenContour, Some(l))]);
    assert_eq!(
        fx.one(RuleId::TabsOnOpenContour).message,
        "No tabs on the open contour on layer 0: it is not closed"
    );
}

// ---------------------------------------------------------------- duplicates

#[test]
fn missing_tool_once_per_op_and_nothing_else() {
    let mut fx = ok();
    let (c, pk) = (fx.contour, fx.pocket);
    fx.op(c).tool = 9;
    fx.op(pk).tool = 9;
    fx.op(pk).depth = 50.0; // op rules need the tool: silent
    let v: Vec<_> = fx.issues().iter().map(|i| (i.rule, i.op, i.item)).collect();
    assert_eq!(
        v,
        [
            (RuleId::MissingTool, Some(c), None),
            (RuleId::MissingTool, Some(pk), None)
        ]
    );
}

#[test]
fn missing_tool_and_item_on_one_op() {
    let mut fx = ok();
    let pk = fx.pocket;
    fx.op(pk).tool = 9;
    fx.op(pk).items.push(999);
    let v: Vec<_> = fx.issues().iter().map(|i| (i.rule, i.item)).collect();
    assert_eq!(
        v,
        [
            (RuleId::MissingTool, None),
            (RuleId::MissingItem, Some(999))
        ]
    );
}

#[test]
fn missing_item_listed_twice_reported_once() {
    let mut fx = ok();
    let pk = fx.pocket;
    fx.op(pk).items.extend([999, 999]);
    assert_eq!(fx.count(RuleId::MissingItem), 1);
}

#[test]
fn duplicate_tool_three_copies_reported_once() {
    let mut fx = ok();
    fx.pr.tools.push(t1());
    fx.pr.tools.push(t1());
    let v = fx.rule(RuleId::DuplicateToolId);
    assert_eq!(v.len(), 1, "{v:#?}");
    assert_eq!(
        (v[0].op, v[0].item, v[0].message.as_str()),
        (None, None, "Tool T1 is embedded more than once")
    );
}

#[test]
fn duplicate_id_three_copies_reported_once() {
    let mut fx = ok();
    for _ in 0..2 {
        let mut dup = fx.pr.items[0].clone();
        dup.id = fx.s;
        fx.pr.items.push(dup);
    }
    let i = fx.one(RuleId::DuplicateId);
    assert_eq!(i.message, format!("Id {} is used more than once", fx.s));
}

#[test]
fn id_counter_message() {
    let mut fx = ok();
    fx.pr.next_id = fx.drill;
    let i = fx.one(RuleId::IdCounter);
    assert_eq!(
        (i.op, i.item, i.message),
        (
            None,
            None,
            format!("Id {} is not below the id counter", fx.drill)
        )
    );
}

#[test]
fn invalid_machine_once_with_many_ops() {
    let mut fx = ok();
    let s = fx.s;
    for _ in 0..50 {
        let id = fx.add_op(contour_kind(Side::Outside), &t1(), vec![s]);
        fx.op(id).depth = 3.0; // not through: no tab warnings
    }
    for (clear, safe, tc, want) in [
        (0.0, 5.0, 40.0, vec!["Clearance Z must be greater than 0"]),
        (-1.0, 5.0, 40.0, vec!["Clearance Z must be greater than 0"]),
        (
            f64::NAN,
            5.0,
            40.0,
            vec!["Clearance Z must be greater than 0"],
        ),
        (
            2.0,
            1.0,
            40.0,
            vec!["Safe Z must be at or above clearance Z"],
        ),
        (
            2.0,
            f64::INFINITY,
            40.0,
            vec![
                "Safe Z must be at or above clearance Z",
                "Tool change Z must be at or above safe Z",
            ],
        ),
        (2.0, 5.0, 5.0 - 1e-10, vec![]),
        (
            2.0,
            5.0,
            4.99,
            vec!["Tool change Z must be at or above safe Z"],
        ),
    ] {
        let mut pr = fx.pr.clone();
        pr.machine.clearance_z = clear;
        pr.machine.safe_z = safe;
        pr.machine.tool_change_z = tc;
        let v = check(&pr, &generate(&pr));
        sane(&v);
        let got: Vec<_> = v
            .iter()
            .filter(|i| i.rule == RuleId::InvalidMachine)
            .map(|i| (i.op, i.message.as_str()))
            .collect();
        let want: Vec<_> = want.into_iter().map(|m| (None, m)).collect();
        assert_eq!(got, want, "{clear} {safe} {tc}");
        // nothing else: the per-op InvalidParam("clearance_z"/"safe_z") is suppressed
        let other: Vec<_> = v
            .iter()
            .filter(|i| i.rule != RuleId::InvalidMachine)
            .collect();
        assert!(other.is_empty(), "{clear} {safe} {tc}: {other:#?}");
    }
}

#[test]
fn invalid_stock_one_per_field() {
    let mut fx = ok();
    fx.pr.stock.width = f64::NAN;
    fx.pr.stock.height = -1.0;
    fx.pr.stock.min_y = f64::INFINITY;
    let v = fx.rule(RuleId::InvalidStock);
    let msgs: Vec<_> = v.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(
        msgs,
        [
            "Stock width must be greater than 0",
            "Stock height must be greater than 0",
            "Stock position Y must be a finite number"
        ]
    );
    assert!(v.iter().all(|i| i.op.is_none() && i.item.is_none()));
}

#[test]
fn same_item_twice_in_pocket_is_not_a_shared_edge() {
    let mut fx = ok();
    let (pk, c2) = (fx.pocket, fx.c2);
    fx.op(pk).items = vec![c2, c2];
    assert_eq!(fx.count(RuleId::SharedEdge), 0);
}

#[test]
fn shared_edge_reported_on_the_item_whose_edge_lies_on_the_other() {
    let mut fx = ok();
    let (pk, s) = (fx.pocket, fx.s);
    let inner = add(&mut fx.pr, Geom::Contour(rect(10.0, 10.0, 25.0, 25.0)));
    // reversed op.items order: still exactly one issue for the pair
    fx.op(pk).items = vec![inner, s];
    let i = fx.one(RuleId::SharedEdge);
    assert_eq!(i.op, Some(pk));
    assert!(i.item == Some(inner) || i.item == Some(s), "{i:?}");
    // three items sharing edges pairwise: review ruling 4f reports only the two nested pairs (s-a, s-b), not a-b side by side
    let mut fx = ok();
    let a = add(&mut fx.pr, Geom::Contour(rect(10.0, 10.0, 25.0, 25.0)));
    let b = add(&mut fx.pr, Geom::Contour(rect(25.0, 10.0, 40.0, 25.0)));
    fx.op(pk).items = vec![s, a, b];
    assert_eq!(fx.count(RuleId::SharedEdge), 2, "{:#?}", fx.issues());
}

#[test]
fn shared_edge_runs_even_when_pocket_fails() {
    let mut fx = ok();
    let (pk, s) = (fx.pocket, fx.s);
    let inner = add(&mut fx.pr, Geom::Contour(rect(10.0, 10.0, 25.0, 25.0)));
    fx.op(pk).items = vec![s, inner];
    fx.op(pk).depth_per_pass = 0.0; // generation fails
    let rules: Vec<_> = fx.issues().iter().map(|i| i.rule).collect();
    assert_eq!(rules, [RuleId::InvalidValue, RuleId::SharedEdge]);
}

// ---------------------------------------------------------------- disabled, mixed, order

#[test]
fn disabled_ops_produce_nothing() {
    let mut fx = ok();
    let (c, pk, d) = (fx.contour, fx.pocket, fx.drill);
    fx.op(c).tool = 9;
    fx.op(pk).items.push(999);
    fx.op(pk).rpm = 0;
    fx.op(pk).depth = 50.0;
    fx.op(d).depth_per_pass = 0.0;
    fx.pr.stock.min_x = 500.0;
    fx.only(&[]);
    assert_eq!(fx.issues(), []);
    // project-level problems still show with every op disabled
    fx.pr.tools.push(t2());
    assert_eq!(
        fx.issues().iter().map(|i| i.rule).collect::<Vec<_>>(),
        [RuleId::DuplicateToolId]
    );
}

#[test]
fn problems_errors_and_notes_all_surface_in_order() {
    let mut fx = ok();
    let (c, pk, d, s) = (fx.contour, fx.pocket, fx.drill, fx.s);
    fx.pr.tools.push(t1()); // project: DuplicateToolId
    fx.pr.next_id = d; // project: IdCounter
    fx.op(c).tabs.placement = TabPlacement::Count { count: 40 }; // note
    fx.op(pk).depth_per_pass = 0.0; // ToolpathError::InvalidParam
    fx.op(d).items.push(999); // Problem::MissingItem (and ToolpathError, not twice)
    let v: Vec<_> = fx.issues().iter().map(|i| (i.rule, i.op, i.item)).collect();
    assert_eq!(
        v,
        [
            (RuleId::DuplicateToolId, None, None),
            (RuleId::IdCounter, None, None),
            (RuleId::TabsReduced, Some(c), Some(s)),
            (RuleId::InvalidValue, Some(pk), None),
            (RuleId::MissingItem, Some(d), Some(999)),
        ]
    );
}

#[test]
fn table_order_within_an_op() {
    let mut fx = ok();
    let pk = fx.pocket;
    let o = fx.op(pk);
    o.tool = 2; // 13 (drill bit), flute 20
    o.depth = 25.0; // 14, 18
    o.rpm = 0; // 15
    o.feed = 100.0 * 25.4; // 16
    o.plunge_feed = 100.0 * 25.4; // 16 again
    o.kind = OpKind::Pocket { stepover_pct: 60.0 }; // 17
    fx.only(&[pk]);
    let v: Vec<_> = fx.issues().iter().map(|i| i.rule).collect();
    let want = [
        RuleId::DrillBitSideCut,
        RuleId::DepthBeyondFlutes,
        RuleId::SpindleOff,
        RuleId::FeedVsTool,
        RuleId::FeedVsTool,
        RuleId::StepoverHigh,
        RuleId::ThroughCut,
    ];
    // OutsideStock or other tp rules may follow; the op-rule prefix is fixed
    assert!(v.starts_with(&want), "{v:?}");
    let feeds: Vec<_> = fx
        .rule(RuleId::FeedVsTool)
        .into_iter()
        .map(|i| i.message)
        .collect();
    assert!(feeds[0].starts_with("Feed ") && feeds[1].starts_with("Plunge feed "));
}

#[test]
fn ops_in_user_order_not_id_order() {
    let mut fx = ok();
    let (c, pk) = (fx.contour, fx.pocket);
    fx.op(c).rpm = 0;
    fx.op(pk).rpm = 0;
    fx.pr.operations.swap(0, 1);
    let v: Vec<_> = fx.issues().iter().map(|i| i.op).collect();
    assert_eq!(v, [Some(pk), Some(c)]);
}

#[test]
fn nan_and_negative_values_do_not_panic_or_duplicate() {
    for v in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let mut fx = ok();
        let (c, pk, d) = (fx.contour, fx.pocket, fx.drill);
        fx.op(c).depth = v;
        fx.op(c).tabs.height = v;
        fx.op(pk).feed = v;
        fx.op(pk).kind = OpKind::Pocket { stepover_pct: v };
        fx.op(d).plunge_feed = v;
        let issues = fx.issues();
        for id in [c, pk, d] {
            assert!(
                issues
                    .iter()
                    .any(|i| i.op == Some(id) && i.rule == RuleId::InvalidValue),
                "{v}: {issues:#?}"
            );
        }
    }
}

#[test]
fn check_with_stale_or_empty_toolpaths_runs_op_rules_only() {
    let mut fx = ok();
    let pk = fx.pocket;
    fx.op(pk).rpm = 0;
    let v = check(&fx.pr, &[]);
    sane(&v);
    assert_eq!(
        v.iter().map(|i| (i.rule, i.op)).collect::<Vec<_>>(),
        [(RuleId::SpindleOff, Some(pk))]
    );
}

// ---------------------------------------------------------------- performance and the Bracket

fn bracket() -> Project {
    let imp = hc_dxf::import_dxf_file(std::path::Path::new(BRACKET), &Default::default()).unwrap();
    let mut pr = Project::new();
    pr.add_dxf(&imp, "bracket.dxf");
    let mut b = BBox::empty();
    for i in &pr.items {
        if let Geom::Contour(c) = &i.geom {
            b = b.union(&c.bbox());
        }
    }
    pr.stock.min_x = b.min.x;
    pr.stock.min_y = b.min.y;
    pr.stock.width = b.width();
    pr.stock.height = b.height();
    pr
}

fn outline_circles(pr: &Project, d: f64) -> Vec<u64> {
    pr.items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter(|i| match &i.geom {
            Geom::Contour(c) => c
                .as_circle()
                .is_some_and(|ci| (2.0 * ci.radius - d).abs() < 1e-6),
            _ => false,
        })
        .map(|i| i.id)
        .collect()
}

fn outer(pr: &Project) -> u64 {
    pr.items
        .iter()
        .filter(|i| i.layer == "Outline")
        .filter_map(|i| match &i.geom {
            Geom::Contour(c) if c.closed => Some((i.id, c.signed_area().abs())),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0
}

/// Spec test 28's job, built in the library (the app test does the same through AppState).
fn bracket_job() -> Project {
    let mut pr = bracket();
    let mut drill = t2();
    drill.diameter = 15.875;
    drill.feed = 100.0;
    let edge = outer(&pr);
    let o = pr.add_operation("Outside", contour_kind(Side::Outside), &t1(), vec![edge]);
    let pocket = [outline_circles(&pr, 76.2), outline_circles(&pr, 44.45)].concat();
    assert_eq!(pocket.len(), 2);
    let pk = pr.add_operation(
        "Pocket",
        OpKind::Pocket { stepover_pct: 0.0 },
        &t1(),
        pocket,
    );
    let holes = outline_circles(&pr, 15.875);
    assert!(!holes.is_empty());
    let d = pr.add_operation("Drill", OpKind::Drill { peck: None }, &drill, holes);
    for op in &mut pr.operations {
        op.depth = if op.id == pk { 3.0 } else { 10.0 };
        if op.id == o {
            op.tabs.enabled = true;
            op.tabs.placement = TabPlacement::Count { count: 4 };
            op.tabs.width = 5.0;
            op.tabs.height = 2.0;
        }
    }
    let _ = d;
    pr
}

#[test]
fn bracket_job_is_clean_in_mm_and_inch() {
    let mut pr = bracket_job();
    let v = check(&pr, &generate(&pr));
    assert_eq!(v, [], "{v:#?}");
    pr.units = Units::Inch;
    assert_eq!(check(&pr, &generate(&pr)), []);
}

#[test]
fn check_is_cheap_on_200_ops() {
    let mut pr = bracket_job();
    let base = pr.operations.clone();
    while pr.operations.len() < 200 {
        let mut o = base[pr.operations.len() % 3].clone();
        o.id = pr.next_id;
        pr.next_id += 1;
        pr.operations.push(o);
    }
    let paths = generate(&pr);
    let t = Instant::now();
    let v = check(&pr, &paths);
    let dt = t.elapsed();
    assert_eq!(v, []);
    assert!(dt < Duration::from_secs(1), "check took {dt:?}");
}

#[test]
fn shared_edge_cost_on_a_big_pocket() {
    // 400 small squares in one pocket (a grid of 1x1 mm cells with gaps): O(n^2) pairs
    let mut fx = ok();
    let pk = fx.pocket;
    let mut ids = Vec::new();
    for i in 0..20 {
        for j in 0..20 {
            let (x, y) = (5.0 + 4.0 * i as f64, 5.0 + 4.0 * j as f64);
            ids.push(add(&mut fx.pr, Geom::Contour(rect(x, y, x + 2.0, y + 2.0))));
        }
    }
    fx.op(pk).items = ids;
    fx.only(&[pk]);
    let t = Instant::now();
    let v = check(&fx.pr, &[]);
    let dt = t.elapsed();
    assert_eq!(v.iter().filter(|i| i.rule == RuleId::SharedEdge).count(), 0);
    assert!(dt < Duration::from_secs(1), "check took {dt:?}");
}
