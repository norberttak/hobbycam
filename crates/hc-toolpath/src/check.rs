//! Sanity checks (M8): load problems, generation errors, toolpath notes and plausibility rules → one Issue list.

use crate::{tab_positions, Move, Toolpath, ToolpathError, ToolpathNote};
use hc_geom::{BBox, Contour, Point, Segment};
use hc_project::{fmt_len, Geom, OpKind, Operation, Problem, Project, Side, ToolKind};
use std::cmp::Ordering;
use std::f64::consts::TAU;

const TOL: f64 = 1e-6;
/// FeedVsTool: op feed more than this factor above or below the tool's (an inch/mm mix-up is 25.4×).
const FEED_FACTOR: f64 = 10.0;
/// ThroughCut: cutting up to this far (mm) into the spoilboard is normal.
const SPOILBOARD: f64 = 1.0;
/// StepoverHigh: percent of the tool diameter.
const STEPOVER_MAX: f64 = 50.0;
/// DrillSize: how much larger (mm) than the hole the drill may be.
const DRILL_TOL: f64 = 0.05;
/// TabOnCorner: tangent turn (degrees) that counts as a corner.
const CORNER_DEG: f64 = 30.0;
/// OutsideStock: margin (mm) on top of the tool radius.
const STOCK_MARGIN: f64 = 1e-3;
/// InvalidMachine: tool_change_z ≥ safe_z tolerance (same as M7).
const MACHINE_TOL: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// The rules of specs/M8.md, in table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleId {
    MissingTool,
    MissingItem,
    DuplicateId,
    DuplicateToolId,
    IdCounter,
    InvalidMachine,
    InvalidStock,
    InvalidValue,
    NoItems,
    OpenContour,
    UnsupportedItem,
    ToolTooLarge,
    DrillBitSideCut,
    DepthBeyondFlutes,
    SpindleOff,
    FeedVsTool,
    StepoverHigh,
    ThroughCut,
    TabsNotThrough,
    TabTooHigh,
    TabsReduced,
    TabsOnOpenContour,
    NoTabsThroughCut,
    TabOnCorner,
    DrillSize,
    SharedEdge,
    OutsideStock,
    PocketUncut,
}

impl RuleId {
    pub fn severity(self) -> Severity {
        use RuleId::*;
        match self {
            MissingTool | MissingItem | DuplicateId | DuplicateToolId | InvalidMachine
            | InvalidStock | InvalidValue | NoItems | OpenContour | UnsupportedItem
            | ToolTooLarge | DrillBitSideCut | DepthBeyondFlutes => Severity::Error,
            _ => Severity::Warning,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    pub rule: RuleId,
    pub op: Option<u64>,
    pub item: Option<u64>,
    /// One sentence without the op name (the GUI adds it). Lengths and feeds via fmt_len in project units.
    pub message: String,
}

fn issue(rule: RuleId, op: Option<u64>, item: Option<u64>, message: String) -> Issue {
    Issue {
        severity: rule.severity(),
        rule,
        op,
        item,
        message,
    }
}

/// `toolpaths` = the output of `generate(project)` (enabled ops, user order). Pure and deterministic.
pub fn check(
    project: &Project,
    toolpaths: &[(u64, Result<Toolpath, ToolpathError>)],
) -> Vec<Issue> {
    let problems = project.validate();
    let mut out = Vec::new();
    project_rules(project, &problems, &mut out);
    for op in project.operations.iter().filter(|o| o.enabled) {
        let res = toolpaths
            .iter()
            .find(|(id, _)| *id == op.id)
            .map(|(_, r)| r);
        op_issues(project, &problems, op, res, &mut out);
    }
    out
}

fn project_rules(project: &Project, problems: &[Problem], out: &mut Vec<Issue>) {
    for p in problems {
        if let Problem::DuplicateId(id) = p {
            out.push(issue(
                RuleId::DuplicateId,
                None,
                None,
                format!("Id {id} is used more than once"),
            ));
        }
    }
    for p in problems {
        if let Problem::DuplicateToolId(t) = p {
            out.push(issue(
                RuleId::DuplicateToolId,
                None,
                None,
                format!("Tool T{t} is embedded more than once"),
            ));
        }
    }
    for p in problems {
        if let Problem::IdNotBelowNextId(id) = p {
            out.push(issue(
                RuleId::IdCounter,
                None,
                None,
                format!("Id {id} is not below the id counter"),
            ));
        }
    }
    let m = &project.machine;
    let mut machine = |msg: &str| {
        out.push(issue(RuleId::InvalidMachine, None, None, msg.to_string()));
    };
    if !m.clearance_z.is_finite() || m.clearance_z <= 0.0 {
        machine("Clearance Z must be greater than 0");
    }
    if !m.safe_z.is_finite() || m.safe_z < m.clearance_z {
        machine("Safe Z must be at or above clearance Z");
    }
    if !m.tool_change_z.is_finite() || m.tool_change_z < m.safe_z - MACHINE_TOL {
        machine("Tool change Z must be at or above safe Z");
    }
    let s = &project.stock;
    for (name, v) in [
        ("width", s.width),
        ("height", s.height),
        ("thickness", s.thickness),
    ] {
        if !v.is_finite() || v <= 0.0 {
            out.push(issue(
                RuleId::InvalidStock,
                None,
                None,
                format!("Stock {name} must be greater than 0"),
            ));
        }
    }
    for (name, v) in [("X", s.min_x), ("Y", s.min_y)] {
        if !v.is_finite() {
            out.push(issue(
                RuleId::InvalidStock,
                None,
                None,
                format!("Stock position {name} must be a finite number"),
            ));
        }
    }
}

/// Rules 8–12 from the generation error; None for the errors that a Problem or InvalidMachine already reports.
fn error_issue(project: &Project, e: &ToolpathError) -> Option<(RuleId, Option<u64>, String)> {
    let the = |id| the(project, id);
    match *e {
        ToolpathError::MissingTool(_) | ToolpathError::MissingItem(_) => None,
        ToolpathError::InvalidParam("clearance_z" | "safe_z") => None,
        ToolpathError::InvalidParam(_) => Some((RuleId::InvalidValue, None, e.to_string())),
        ToolpathError::NoItems => Some((RuleId::NoItems, None, e.to_string())),
        ToolpathError::OpenContour(id) => Some((
            RuleId::OpenContour,
            Some(id),
            format!("Needs closed contours, but got {}", the(id)),
        )),
        ToolpathError::UnsupportedItem(id) => Some((
            RuleId::UnsupportedItem,
            Some(id),
            format!("Cannot use {} in this operation", the(id)),
        )),
        ToolpathError::OffsetVanished(id) => Some((
            RuleId::ToolTooLarge,
            Some(id),
            format!("Tool too large for {}", the(id)),
        )),
    }
}

/// What the user sees of an item: "circle Ø15.875 mm on layer Outline", "open contour on layer 0",
/// "point on layer 0", or "a deleted item (id 999)" (ids are never shown otherwise).
pub fn describe(project: &Project, id: u64) -> String {
    let Some(item) = project.items.iter().find(|i| i.id == id) else {
        return format!("a deleted item (id {id})");
    };
    let what = match &item.geom {
        Geom::Point(_) => "point".to_string(),
        Geom::Contour(c) => match c.as_circle() {
            Some(ci) => format!("circle Ø{}", fmt_len(2.0 * ci.radius, project.units)),
            None if c.closed => "closed contour".to_string(),
            None => "open contour".to_string(),
        },
    };
    format!("{what} on layer {}", item.layer)
}

/// `describe` with "the" in front of an existing item.
fn the(project: &Project, id: u64) -> String {
    let d = describe(project, id);
    if project.items.iter().any(|i| i.id == id) {
        format!("the {d}")
    } else {
        d
    }
}

fn contour(project: &Project, id: u64) -> Option<&Contour> {
    project
        .items
        .iter()
        .find(|i| i.id == id)
        .and_then(|i| match &i.geom {
            Geom::Contour(c) => Some(c),
            Geom::Point(_) => None,
        })
}

fn op_issues(
    project: &Project,
    problems: &[Problem],
    op: &Operation,
    res: Option<&Result<Toolpath, ToolpathError>>,
    out: &mut Vec<Issue>,
) {
    let oid = Some(op.id);
    for p in problems {
        if let Problem::MissingTool { op: o, tool } = *p {
            if o == op.id {
                out.push(issue(
                    RuleId::MissingTool,
                    oid,
                    None,
                    format!("Uses tool T{tool}, which is not in the project"),
                ));
            }
        }
    }
    for p in problems {
        if let Problem::MissingItem { op: o, item } = *p {
            if o == op.id {
                out.push(issue(
                    RuleId::MissingItem,
                    oid,
                    Some(item),
                    format!("Refers to {}, which does not exist", the(project, item)),
                ));
            }
        }
    }
    if let Some(Err(e)) = res {
        if let Some((rule, item, msg)) = error_issue(project, e) {
            out.push(issue(rule, oid, item, msg));
        }
    }
    let Some(tool) = project.tools.iter().find(|t| t.id == op.tool) else {
        return;
    };
    let tp = match res {
        Some(Ok(tp)) => Some(tp),
        _ => None,
    };
    let len = |v: f64| fmt_len(v, project.units);
    let mut add = |rule, item, msg: String| out.push(issue(rule, oid, item, msg));
    let thickness = project.stock.thickness;
    let closed: Vec<(u64, &Contour)> = op
        .items
        .iter()
        .filter_map(|&id| contour(project, id).filter(|c| c.closed).map(|c| (id, c)))
        .collect();
    let is_contour = matches!(op.kind, OpKind::Contour { .. });

    // 13 – 20: op rules
    if !matches!(op.kind, OpKind::Drill { .. }) && tool.kind == ToolKind::Drill {
        add(
            RuleId::DrillBitSideCut,
            None,
            format!("T{} is a drill bit and cannot mill", tool.id),
        );
    }
    // NaN-safe: an incomparable depth or flute length fires too
    let within = op.depth.partial_cmp(&(tool.flute_length + TOL));
    if !matches!(within, Some(Ordering::Less | Ordering::Equal)) {
        add(
            RuleId::DepthBeyondFlutes,
            None,
            format!(
                "Depth {} is deeper than the flute length {} of T{}",
                len(op.depth),
                len(tool.flute_length),
                tool.id
            ),
        );
    }
    if op.rpm == 0 {
        add(RuleId::SpindleOff, None, "Spindle speed is 0 (S0)".into());
    }
    let feeds = [
        ("Feed", op.feed, tool.feed),
        ("Plunge feed", op.plunge_feed, tool.plunge_feed),
    ];
    let skip = usize::from(matches!(op.kind, OpKind::Drill { .. }));
    for &(name, v, t) in &feeds[skip..] {
        // non-finite or ≤ 0 values are InvalidValue already
        let usable = |x: f64| x.is_finite() && x > 0.0;
        if usable(v) && usable(t) && (v > FEED_FACTOR * t || v < t / FEED_FACTOR) {
            add(
                RuleId::FeedVsTool,
                None,
                format!(
                    "{name} {}/min is far from the tool's {}/min",
                    len(v),
                    len(t)
                ),
            );
        }
    }
    if let OpKind::Pocket { stepover_pct } = op.kind {
        if stepover_pct > STEPOVER_MAX + 1e-9 {
            add(
                RuleId::StepoverHigh,
                None,
                format!("Stepover {stepover_pct:.0} % may leave material between rings"),
            );
        }
    }
    if op.depth > thickness + SPOILBOARD {
        add(
            RuleId::ThroughCut,
            None,
            format!(
                "Cuts {} into the spoilboard (depth {}, stock {})",
                len(op.depth - thickness),
                len(op.depth),
                len(thickness)
            ),
        );
    }
    let tabs = is_contour && op.tabs.enabled;
    if tabs && op.depth < thickness - TOL {
        add(
            RuleId::TabsNotThrough,
            None,
            "Tabs are enabled but the cut does not go through the stock".into(),
        );
    }
    if tabs && op.tabs.height >= op.depth - TOL {
        add(
            RuleId::TabTooHigh,
            None,
            format!(
                "Tab height {} is not below the depth {}",
                len(op.tabs.height),
                len(op.depth)
            ),
        );
    }

    // 21 – 25: toolpath rules
    if let Some(tp) = tp {
        for n in &tp.notes {
            if let ToolpathNote::TabsReduced {
                item,
                placed,
                requested,
            } = *n
            {
                add(
                    RuleId::TabsReduced,
                    Some(item),
                    format!(
                        "Only {placed} of {requested} tabs fit on {}",
                        the(project, item)
                    ),
                );
            }
        }
        for n in &tp.notes {
            if let ToolpathNote::TabsOnOpenContour(item) = *n {
                add(
                    RuleId::TabsOnOpenContour,
                    Some(item),
                    format!("No tabs on {}: it is not closed", the(project, item)),
                );
            }
        }
        // amendment A1: Inside through-cuts free the waste slug of a hole, which is normal
        if matches!(
            op.kind,
            OpKind::Contour {
                side: Side::Outside | Side::On,
                ..
            }
        ) && op.depth >= thickness - TOL
        {
            for &(id, c) in &closed {
                if !op.tabs.enabled || tab_positions(c, &op.tabs, id, tool.diameter).0.is_empty() {
                    add(
                        RuleId::NoTabsThroughCut,
                        Some(id),
                        "The part may come loose: through-cut without tabs".into(),
                    );
                }
            }
        }
        if tabs {
            for &(id, c) in &closed {
                let l = c.length();
                let corners = corners(c);
                let (pos, _) = tab_positions(c, &op.tabs, id, tool.diameter);
                for (k, t) in pos.iter().enumerate() {
                    let s = t.rem_euclid(1.0) * l;
                    if corners
                        .iter()
                        .any(|&a| loop_dist(a, s, l) <= op.tabs.width / 2.0 + TOL)
                    {
                        add(
                            RuleId::TabOnCorner,
                            Some(id),
                            format!(
                                "Tab {} sits on a corner and may be weak or misplaced",
                                k + 1
                            ),
                        );
                    }
                }
            }
        }
        if matches!(op.kind, OpKind::Drill { .. }) {
            for &id in &op.items {
                let Some(ci) = contour(project, id).and_then(Contour::as_circle) else {
                    continue;
                };
                let d = 2.0 * ci.radius;
                // only oversize ruins the part; a smaller bit is a pilot / tap / clearance case
                if tool.diameter > d + DRILL_TOL {
                    add(
                        RuleId::DrillSize,
                        Some(id),
                        format!("Hole Ø{}, drill Ø{}", len(d), len(tool.diameter)),
                    );
                }
            }
        }
    }

    // 26: on the existing items, even when the pocket failed
    if matches!(op.kind, OpKind::Pocket { .. }) {
        for (i, &(a, ca)) in closed.iter().enumerate() {
            for &(b, cb) in &closed[i + 1..] {
                // nested only (the island case); side-by-side pockets are cut separately.
                // One issue per pair, on the inner item (the island), so two islands in one boundary stay distinct.
                let (ba, bb) = (ca.bbox(), cb.bbox());
                let pair = if a == b || !(touches(ca, cb) || touches(cb, ca)) {
                    None
                } else if ba.contains_bbox(&bb) {
                    Some((b, a))
                } else if bb.contains_bbox(&ba) {
                    Some((a, b))
                } else {
                    None
                };
                if let Some((x, y)) = pair {
                    add(
                        RuleId::SharedEdge,
                        Some(x),
                        format!(
                            "The {} touches the {}: the pocket treats them as one wall",
                            describe(project, x),
                            describe(project, y)
                        ),
                    );
                }
            }
        }
    }

    // 27
    if let Some(tp) = tp {
        let r = tool.diameter / 2.0;
        let worst = beyond_stock(project, tp);
        if worst > r + STOCK_MARGIN {
            add(
                RuleId::OutsideStock,
                None,
                format!("Cuts up to {} outside the stock", len(worst + r)),
            );
        }
    }

    // 28
    if let Some(tp) = tp {
        for n in &tp.notes {
            if let ToolpathNote::PocketUncut(item) = *n {
                add(
                    RuleId::PocketUncut,
                    Some(item),
                    format!(
                        "Parts of {} are narrower than the tool and stay uncut",
                        the(project, item)
                    ),
                );
            }
        }
    }
}

/// Distance between two arc-length positions on a closed loop of length `l`.
fn loop_dist(a: f64, b: f64, l: f64) -> f64 {
    let d = (a - b).abs().rem_euclid(l);
    d.min(l - d)
}

/// Direction of travel at the start (or end) of a segment, not normalised.
pub(crate) fn tangent(s: &Segment, at_end: bool) -> Point {
    match *s {
        Segment::Line { start, end } => end - start,
        Segment::Arc {
            start_angle, sweep, ..
        } => {
            let a = if at_end {
                start_angle + sweep
            } else {
                start_angle
            };
            Point::new(-a.sin(), a.cos()) * sweep.signum()
        }
    }
}

/// Arc-length positions of the segment junctions where the tangent turns by more than CORNER_DEG.
fn corners(c: &Contour) -> Vec<f64> {
    let n = c.segments.len();
    let mut out = Vec::new();
    let mut acc = 0.0;
    for (i, seg) in c.segments.iter().enumerate() {
        if i > 0 || c.closed {
            let a = tangent(&c.segments[(i + n - 1) % n], true);
            let b = tangent(seg, false);
            if a.cross(b).atan2(a.dot(b)).abs() > CORNER_DEG.to_radians() {
                out.push(acc);
            }
        }
        acc += seg.length();
    }
    out
}

/// A segment midpoint of `a` lies on `b`.
fn touches(a: &Contour, b: &Contour) -> bool {
    a.segments
        .iter()
        .any(|s| b.distance_to(s.midpoint()) <= TOL)
}

/// Largest per-axis distance (mm) of a tool-centre position of a feed move below Z0 outside the stock rect; 0 when inside.
fn beyond_stock(project: &Project, tp: &Toolpath) -> f64 {
    let s = &project.stock;
    let (x1, y1) = (s.min_x + s.width, s.min_y + s.height);
    let excess = |b: BBox| {
        (s.min_x - b.min.x)
            .max(b.max.x - x1)
            .max(s.min_y - b.min.y)
            .max(b.max.y - y1)
    };
    let pt = |p: Point| BBox { min: p, max: p };
    let mut worst = 0.0f64;
    let mut pos: Option<Point> = None;
    for m in &tp.moves {
        let (cut, next) = match *m {
            Move::Rapid(p) => (None, p.xy()),
            Move::Linear { to, .. } => ((to.z < -TOL).then(|| pt(to.xy())), to.xy()),
            Move::Arc {
                to, center, ccw, ..
            } => {
                let b = pos
                    .filter(|_| to.z < -TOL)
                    .map(|from| arc(from, to.xy(), center, ccw).bbox());
                (b, to.xy())
            }
            Move::Drill { at, z_bottom, .. } => ((z_bottom < -TOL).then(|| pt(at)), at),
        };
        if let Some(b) = cut {
            worst = worst.max(excess(b));
        }
        pos = Some(next);
    }
    worst
}

/// The arc of a `Move::Arc` from `from`.
fn arc(from: Point, to: Point, center: Point, ccw: bool) -> Segment {
    let ang = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let (a0, a1) = (ang(from), ang(to));
    let mut sweep = if ccw {
        (a1 - a0).rem_euclid(TAU)
    } else {
        -(a0 - a1).rem_euclid(TAU)
    };
    if sweep.abs() < 1e-12 {
        sweep = if ccw { TAU } else { -TAU };
    }
    Segment::Arc {
        center,
        radius: from.distance(center),
        start_angle: a0,
        sweep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_geom::Circle;

    #[test]
    fn describe_items() {
        use hc_project::{Item, Source, Units};
        let mut pr = Project::new();
        let mut push = |id, geom| {
            pr.items.push(Item {
                id,
                layer: "Outline".into(),
                source: Source::Drawn,
                geom,
            })
        };
        let circle = Circle {
            center: Point::new(0.0, 0.0),
            radius: 7.9375,
        }
        .to_contour();
        let open = Contour::from_bulge_vertices(
            &[(Point::new(0.0, 0.0), 0.0), (Point::new(1.0, 0.0), 0.0)],
            false,
        );
        let mut closed = open.clone();
        closed.segments.push(Segment::Line {
            start: Point::new(1.0, 0.0),
            end: Point::new(0.0, 1.0),
        });
        closed.closed = true;
        push(1, Geom::Contour(circle));
        push(2, Geom::Contour(open));
        push(3, Geom::Contour(closed));
        push(4, Geom::Point(Point::new(1.0, 1.0)));
        assert_eq!(describe(&pr, 1), "circle Ø15.875 mm on layer Outline");
        assert_eq!(describe(&pr, 2), "open contour on layer Outline");
        assert_eq!(describe(&pr, 3), "closed contour on layer Outline");
        assert_eq!(describe(&pr, 4), "point on layer Outline");
        assert_eq!(describe(&pr, 999), "a deleted item (id 999)");
        assert_eq!(the(&pr, 4), "the point on layer Outline");
        assert_eq!(the(&pr, 999), "a deleted item (id 999)");
        pr.units = Units::Inch;
        assert_eq!(describe(&pr, 1), "circle Ø0.6250 in on layer Outline");
    }

    #[test]
    fn severity_order_and_mapping() {
        assert!(Severity::Error > Severity::Warning);
        assert_eq!(RuleId::DepthBeyondFlutes.severity(), Severity::Error);
        assert_eq!(RuleId::IdCounter.severity(), Severity::Warning);
        assert_eq!(RuleId::SpindleOff.severity(), Severity::Warning);
    }

    #[test]
    fn corners_of_square_and_circle() {
        let sq = Contour::from_bulge_vertices(
            &[
                (Point::new(0.0, 0.0), 0.0),
                (Point::new(10.0, 0.0), 0.0),
                (Point::new(10.0, 10.0), 0.0),
                (Point::new(0.0, 10.0), 0.0),
            ],
            true,
        );
        assert_eq!(corners(&sq), [0.0, 10.0, 20.0, 30.0]);
        let mut open = sq.clone();
        open.closed = false;
        assert_eq!(corners(&open), [10.0, 20.0, 30.0]);
        let c = Circle {
            center: Point::new(0.0, 0.0),
            radius: 5.0,
        }
        .to_contour();
        assert!(corners(&c).is_empty());
        // a rounded corner (line, tangent arc, line) has no corner
        let rounded = Contour::from_bulge_vertices(
            &[
                (Point::new(0.0, 0.0), 0.0),
                (Point::new(10.0, 0.0), (std::f64::consts::FRAC_PI_8).tan()),
                (Point::new(12.0, 2.0), 0.0),
                (Point::new(12.0, 10.0), 0.0),
            ],
            false,
        );
        assert!(corners(&rounded).is_empty());
    }

    #[test]
    fn arc_reconstruction_and_loop_dist() {
        let c = Point::new(0.0, 0.0);
        let s = arc(Point::new(1.0, 0.0), Point::new(-1.0, 0.0), c, true);
        let b = s.bbox();
        assert!((b.max.y - 1.0).abs() < 1e-12 && b.min.y.abs() < 1e-12);
        let s = arc(Point::new(1.0, 0.0), Point::new(-1.0, 0.0), c, false);
        assert!((s.bbox().min.y + 1.0).abs() < 1e-12);
        assert!((loop_dist(1.0, 99.0, 100.0) - 2.0).abs() < 1e-12);
        assert!((loop_dist(10.0, 30.0, 100.0) - 20.0).abs() < 1e-12);
    }
}
