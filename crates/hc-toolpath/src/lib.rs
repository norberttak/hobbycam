//! Operations -> toolpaths (move lists). Units mm and mm/min, Z0 = stock top, cutting Z negative.

mod check;
mod offset;
mod tabs;

pub use check::{check, describe, Issue, RuleId, Severity};
pub use offset::{offset_closed, pocket_rings};
pub use tabs::tab_positions;

use hc_geom::{containment_tree, Contour, Orientation, Point, Segment};
use hc_project::{Direction, Geom, Machine, OpKind, Operation, Project, Side, TabPlacement, Tool};
use offset::{split_large_arcs, MAX_LEVELS};
use std::fmt;
use tabs::Intervals;

/// (lifted intervals, z_tab, plunge_feed) for `cut_pass`.
type Lift<'a> = (&'a [(f64, f64)], f64, f64);

/// Positions closer than this (mm) count as the same point.
pub(crate) const POS_TOL: f64 = 1e-6;
/// Upper limit for depth/depth_per_pass and the number of pocket levels.
const MAX_PASSES: f64 = MAX_LEVELS as f64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3 {
    fn at(p: Point, z: f64) -> Point3 {
        Point3 { x: p.x, y: p.y, z }
    }

    pub fn xy(&self) -> Point {
        Point::new(self.x, self.y)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    /// G0; changes Z only or XY only, never both.
    Rapid(Point3),
    /// G1
    Linear { to: Point3, feed: f64 },
    /// G2/G3 in XY, to.z == current z, |sweep| <= π.
    Arc {
        to: Point3,
        center: Point,
        ccw: bool,
        feed: f64,
    },
    /// G81/G83, G98: returns to the Z it started at.
    Drill {
        at: Point,
        z_bottom: f64,
        peck: Option<f64>,
        r_plane: f64,
        feed: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toolpath {
    pub op_id: u64,
    pub tool_id: u32,
    pub rpm: u32,
    pub moves: Vec<Move>,
    /// Non-fatal remarks (M8 turns them into warnings).
    pub notes: Vec<ToolpathNote>,
}

/// Something the user should know about a toolpath that was still generated.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolpathNote {
    TabsReduced {
        item: u64,
        placed: usize,
        requested: usize,
    },
    TabsOnOpenContour(u64),
}

impl fmt::Display for ToolpathNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolpathNote::TabsReduced {
                item,
                placed,
                requested,
            } => write!(f, "Item {item}: only {placed} of {requested} tabs fit"),
            ToolpathNote::TabsOnOpenContour(item) => write!(f, "Item {item} is open: no tabs"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolpathError {
    MissingTool(u32),
    MissingItem(u64),
    NoItems,
    /// Field name, e.g. "depth_per_pass".
    InvalidParam(&'static str),
    /// Pocket / Contour Inside|Outside on an open contour.
    OpenContour(u64),
    /// A Point in Engrave/Contour/Pocket, a non-circle contour in Drill.
    UnsupportedItem(u64),
    /// Tool too large for this item (inside contour / pocket boundary).
    OffsetVanished(u64),
}

impl fmt::Display for ToolpathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolpathError::MissingTool(t) => write!(f, "Tool T{t} is missing"),
            ToolpathError::MissingItem(id) => write!(f, "Item {id} does not exist"),
            ToolpathError::NoItems => write!(f, "No geometry assigned"),
            ToolpathError::InvalidParam(name) => write!(f, "Invalid value: {name}"),
            ToolpathError::OpenContour(id) => {
                write!(f, "Needs closed contours (item {id} is open)")
            }
            ToolpathError::UnsupportedItem(id) => {
                write!(f, "Item {id} cannot be used by this operation")
            }
            ToolpathError::OffsetVanished(id) => write!(f, "Tool too large for item {id}"),
        }
    }
}

impl std::error::Error for ToolpathError {}

/// Enabled ops in user order (disabled skipped), one entry per op.
pub fn generate(project: &Project) -> Vec<(u64, Result<Toolpath, ToolpathError>)> {
    project
        .operations
        .iter()
        .filter(|o| o.enabled)
        .map(|o| (o.id, generate_op(project, o)))
        .collect()
}

/// Pass Z values: n = ceil(depth/dpp) equal steps, deepest last.
pub fn depth_levels(depth: f64, depth_per_pass: f64) -> Vec<f64> {
    let n = (depth / depth_per_pass - 1e-9).ceil().max(1.0) as usize;
    (1..=n).map(|i| -depth * i as f64 / n as f64).collect()
}

fn validate(m: &Machine, op: &Operation) -> Result<(), ToolpathError> {
    use ToolpathError::InvalidParam as E;
    let drill = matches!(op.kind, OpKind::Drill { .. });
    let bad = |v: f64| !v.is_finite() || v <= 0.0;
    if !m.clearance_z.is_finite() || m.clearance_z <= 0.0 {
        return Err(E("clearance_z"));
    }
    if !m.safe_z.is_finite() || m.safe_z < m.clearance_z {
        return Err(E("safe_z"));
    }
    if bad(op.depth) {
        return Err(E("depth"));
    }
    if !drill && (bad(op.depth_per_pass) || op.depth / op.depth_per_pass > MAX_PASSES) {
        return Err(E("depth_per_pass"));
    }
    if !drill && bad(op.feed) {
        return Err(E("feed"));
    }
    if bad(op.plunge_feed) {
        return Err(E("plunge_feed"));
    }
    match op.kind {
        OpKind::Pocket { stepover_pct } if bad(stepover_pct) || stepover_pct > 100.0 => {
            return Err(E("stepover_pct"))
        }
        OpKind::Drill { peck: Some(p) } if bad(p) => return Err(E("peck")),
        _ => {}
    }
    let t = &op.tabs;
    if matches!(op.kind, OpKind::Contour { .. }) && t.enabled {
        if bad(t.width) {
            return Err(E("tab_width"));
        }
        if bad(t.height) {
            return Err(E("tab_height"));
        }
        if matches!(t.placement, TabPlacement::Spacing { distance } if bad(distance)) {
            return Err(E("tab_spacing"));
        }
        if t.manual
            .iter()
            .flat_map(|m| &m.positions)
            .any(|p| !p.is_finite())
        {
            return Err(E("tabs"));
        }
    }
    Ok(())
}

/// One feature: the moves after `Rapid(start @ safe_z)`, ending at `end`.
struct Feature {
    start: Point,
    end: Point,
    /// Containment depth among the op's closed contours (deeper is cut first).
    depth: usize,
    moves: Vec<Move>,
    /// Retract to safe_z after the moves (drill cycles return by themselves).
    retract: bool,
}

pub fn generate_op(project: &Project, op: &Operation) -> Result<Toolpath, ToolpathError> {
    let m = &project.machine;
    validate(m, op)?;
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or(ToolpathError::MissingTool(op.tool))?;
    let needs_radius = matches!(op.kind, OpKind::Contour { .. } | OpKind::Pocket { .. });
    if needs_radius && (!tool.diameter.is_finite() || tool.diameter <= 0.0) {
        return Err(ToolpathError::InvalidParam("diameter"));
    }
    if op.items.is_empty() {
        return Err(ToolpathError::NoItems);
    }
    let items = op
        .items
        .iter()
        .map(|&id| {
            project
                .items
                .iter()
                .find(|i| i.id == id)
                .map(|i| (id, &i.geom))
                .ok_or(ToolpathError::MissingItem(id))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut notes = Vec::new();
    let features = match op.kind {
        OpKind::Drill { peck } => drill_features(m, op, peck, &items)?,
        _ => cut_features(m, op, tool, &items, &mut notes)?,
    };

    let mut moves = Vec::new();
    for f in order(features) {
        moves.push(Move::Rapid(Point3::at(f.start, m.safe_z)));
        moves.extend(f.moves);
        if f.retract {
            moves.push(Move::Rapid(Point3::at(f.end, m.safe_z)));
        }
    }
    Ok(Toolpath {
        op_id: op.id,
        tool_id: op.tool,
        rpm: op.rpm,
        moves,
        notes,
    })
}

/// Deepest containment level first, then nearest neighbour (by feature start) from the previous end, first from (0,0).
// shortcut: open paths are never flipped to start at their nearer end; upgrade if air moves between engravings annoy
fn order(mut rest: Vec<Feature>) -> Vec<Feature> {
    let mut out = Vec::with_capacity(rest.len());
    let mut pos = Point::new(0.0, 0.0);
    while let Some(max_depth) = rest.iter().map(|f| f.depth).max() {
        let i = rest
            .iter()
            .enumerate()
            .filter(|(_, f)| f.depth == max_depth)
            .min_by(|(_, a), (_, b)| a.start.distance(pos).total_cmp(&b.start.distance(pos)))
            .map(|(i, _)| i)
            .expect("max depth exists");
        let f = rest.remove(i);
        pos = f.end;
        out.push(f);
    }
    out
}

fn drill_features(
    m: &Machine,
    op: &Operation,
    peck: Option<f64>,
    items: &[(u64, &Geom)],
) -> Result<Vec<Feature>, ToolpathError> {
    items
        .iter()
        .map(|&(id, g)| {
            let at = match g {
                Geom::Point(p) => *p,
                Geom::Contour(c) => {
                    c.as_circle()
                        .ok_or(ToolpathError::UnsupportedItem(id))?
                        .center
                }
            };
            Ok(Feature {
                start: at,
                end: at,
                depth: 0,
                moves: vec![Move::Drill {
                    at,
                    z_bottom: -op.depth,
                    peck,
                    r_plane: m.clearance_z,
                    feed: op.plunge_feed,
                }],
                retract: false,
            })
        })
        .collect()
}

/// Engrave / Contour / Pocket: the paths of each feature (cut in this order per depth level).
fn cut_features(
    m: &Machine,
    op: &Operation,
    tool: &Tool,
    items: &[(u64, &Geom)],
    notes: &mut Vec<ToolpathNote>,
) -> Result<Vec<Feature>, ToolpathError> {
    let mut contours: Vec<(u64, &Contour)> = Vec::with_capacity(items.len());
    for &(id, g) in items {
        match g {
            Geom::Point(_) => return Err(ToolpathError::UnsupportedItem(id)),
            Geom::Contour(c) if c.segments.is_empty() => {
                return Err(ToolpathError::UnsupportedItem(id))
            }
            Geom::Contour(c) => contours.push((id, c)),
        }
    }
    let r = tool.diameter / 2.0;
    let all: Vec<Contour> = contours.iter().map(|(_, c)| (*c).clone()).collect();
    let tree = containment_tree(&all);

    // (containment depth, paths with their lifted tab intervals)
    let mut groups: Vec<(usize, Vec<(Contour, Intervals)>)> = Vec::new();
    let no_tabs = |paths: Vec<Contour>| paths.into_iter().map(|p| (p, Vec::new())).collect();
    match op.kind {
        OpKind::Engrave => {
            for (k, (_, c)) in contours.iter().enumerate() {
                groups.push((tree[k].depth, no_tabs(vec![(*c).clone()])));
            }
        }
        OpKind::Contour { side, direction } => {
            for (k, &(id, c)) in contours.iter().enumerate() {
                if !c.closed && side != Side::On {
                    return Err(ToolpathError::OpenContour(id));
                }
                let climb = direction == Direction::Climb;
                let paths = match side {
                    Side::On if !c.closed => {
                        let mut c = c.clone();
                        if !climb {
                            c.reverse();
                        }
                        vec![c]
                    }
                    Side::On => vec![oriented(c.clone(), climb, Orientation::Cw)],
                    Side::Outside | Side::Inside => {
                        let (dist, climb_dir) = if side == Side::Outside {
                            (r, Orientation::Cw)
                        } else {
                            (-r, Orientation::Ccw)
                        };
                        let loops = offset_closed(c, dist);
                        if loops.is_empty() {
                            return Err(ToolpathError::OffsetVanished(id));
                        }
                        loops
                            .into_iter()
                            .map(|l| oriented(l, climb, climb_dir))
                            .collect()
                    }
                };
                let paths = if !op.tabs.enabled {
                    no_tabs(paths)
                } else if !c.closed {
                    notes.push(ToolpathNote::TabsOnOpenContour(id));
                    no_tabs(paths)
                } else {
                    let (pos, note) = tab_positions(c, &op.tabs, id, tool.diameter);
                    notes.extend(note);
                    tabs::lift_paths(c, &pos, paths, op.tabs.width + tool.diameter)?
                };
                groups.push((tree[k].depth, paths));
            }
        }
        OpKind::Pocket { stepover_pct } => {
            if let Some(&(id, _)) = contours.iter().find(|(_, c)| !c.closed) {
                return Err(ToolpathError::OpenContour(id));
            }
            let step = stepover_pct / 100.0 * tool.diameter;
            for (k, &(id, b)) in contours.iter().enumerate() {
                if tree[k].depth % 2 == 1 {
                    continue;
                }
                let bb = b.bbox();
                if bb.width().max(bb.height()) / 2.0 / step > MAX_PASSES {
                    return Err(ToolpathError::InvalidParam("stepover_pct"));
                }
                let islands: Vec<Contour> = tree
                    .iter()
                    .filter(|n| n.parent == Some(k))
                    .map(|n| contours[n.contour].1.clone())
                    .collect();
                let levels = pocket_rings(b, &islands, r, step);
                if levels.is_empty() {
                    return Err(ToolpathError::OffsetVanished(id));
                }
                // innermost level first, the wall ring last
                groups.push((
                    tree[k].depth,
                    no_tabs(levels.into_iter().rev().flatten().collect()),
                ));
            }
        }
        OpKind::Drill { .. } => unreachable!("drill handled by drill_features"),
    }

    let levels = depth_levels(op.depth, op.depth_per_pass);
    Ok(groups
        .into_iter()
        .map(|(depth, paths)| {
            let paths: Vec<(Contour, Vec<(f64, f64)>)> = paths
                .iter()
                .map(|(c, iv)| (split_large_arcs(c), iv.clone()))
                .collect();
            let start = paths[0].0.start();
            let mut moves = vec![Move::Rapid(Point3::at(start, m.clearance_z))];
            let end = emit_levels(&mut moves, &paths, &levels, m, op);
            Feature {
                start,
                end,
                depth,
                moves,
                retract: true,
            }
        })
        .collect())
}

/// Climb ⇔ material on the right of travel; `climb_dir` is the orientation that gives climb.
fn oriented(mut c: Contour, climb: bool, climb_dir: Orientation) -> Contour {
    let o = match (climb, climb_dir) {
        (true, o) => o,
        (false, Orientation::Cw) => Orientation::Ccw,
        (false, Orientation::Ccw) => Orientation::Cw,
    };
    c.set_orientation(o);
    c
}

/// All depth levels of one feature, starting at paths[0].start() @ clearance_z. Returns the end XY.
fn emit_levels(
    out: &mut Vec<Move>,
    paths: &[(Contour, Vec<(f64, f64)>)],
    levels: &[f64],
    m: &Machine,
    op: &Operation,
) -> Point {
    let mut pos = paths[0].0.start();
    // a single open path zig-zags: every level runs back from where the previous one ended
    let zigzag = paths.len() == 1 && !paths[0].0.closed;
    // h > depth: the tab reaches the stock top, never lift above Z0 (M8 warns)
    let z_tab = (-op.depth + op.tabs.height).min(0.0);
    for (i, &z) in levels.iter().enumerate() {
        for (path, iv) in paths {
            let mut path = path.clone();
            if zigzag && i % 2 == 1 {
                path.reverse();
            }
            let start = path.start();
            if !start.approx_eq(pos, POS_TOL) {
                // shortcut: no linking at depth, every ring re-enters from clearance_z; upgrade with M6/M9 if air time annoys (links must not cross islands)
                out.push(Move::Rapid(Point3::at(pos, m.clearance_z)));
                out.push(Move::Rapid(Point3::at(start, m.clearance_z)));
            }
            out.push(Move::Linear {
                to: Point3::at(start, z),
                feed: op.plunge_feed,
            });
            cut_pass(out, &path, z, op.feed, Some((iv, z_tab, op.plunge_feed)));
            pos = path.end();
        }
    }
    pos
}

fn seg_move(s: &Segment, z: f64, feed: f64) -> Move {
    let to = Point3::at(s.end(), z);
    match *s {
        Segment::Line { .. } => Move::Linear { to, feed },
        Segment::Arc { center, sweep, .. } => Move::Arc {
            to,
            center,
            ccw: sweep > 0.0,
            feed,
        },
    }
}

/// All contour/pocket cutting at one Z goes through here. `lift` = (intervals, z_tab, plunge_feed):
/// a pass below z_tab goes straight up to z_tab over each interval and straight back down after it.
fn cut_pass(out: &mut Vec<Move>, path: &Contour, z: f64, feed: f64, lift: Option<Lift<'_>>) {
    let Some((iv, z_tab, plunge)) = lift.filter(|&(iv, zt, _)| !iv.is_empty() && z < zt - 1e-9)
    else {
        out.extend(path.segments.iter().map(|s| seg_move(s, z, feed)));
        return;
    };
    // shortcut: rectangular tabs (vertical up/down at plunge feed), replace with a ramp of length h/tan(angle) inside the gap when ramped tabs are wanted
    let mut up = false;
    for (piece, lifted) in tabs::split_pieces(path, iv) {
        if lifted != up {
            up = lifted;
            let to_z = if up { z_tab } else { z };
            out.push(Move::Linear {
                to: Point3::at(piece.start(), to_z),
                feed: plunge,
            });
        }
        out.push(seg_move(&piece, if up { z_tab } else { z }, feed));
    }
    if up {
        out.push(Move::Linear {
            to: Point3::at(path.end(), z),
            feed: plunge,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(x: f64, depth: usize) -> Feature {
        let p = Point::new(x, 0.0);
        Feature {
            start: p,
            end: p,
            depth,
            moves: Vec::new(),
            retract: false,
        }
    }

    #[test]
    fn order_deepest_first_then_nearest() {
        let xs = |v: Vec<Feature>| v.iter().map(|f| f.start.x).collect::<Vec<_>>();
        let got = order(vec![
            feature(30.0, 0),
            feature(-5.0, 0),
            feature(50.0, 1),
            feature(10.0, 0),
            feature(40.0, 1),
        ]);
        // depth 1 from (0,0): 40 then 50; depth 0 from 50: 30, 10, -5
        assert_eq!(xs(got), [40.0, 50.0, 30.0, 10.0, -5.0]);
        assert!(order(Vec::new()).is_empty());
    }

    #[test]
    fn oriented_table() {
        let sq = Contour::from_bulge_vertices(
            &[
                (Point::new(0.0, 0.0), 0.0),
                (Point::new(1.0, 0.0), 0.0),
                (Point::new(1.0, 1.0), 0.0),
            ],
            true,
        );
        let o = |climb, d| oriented(sq.clone(), climb, d).orientation();
        assert_eq!(o(true, Orientation::Cw), Some(Orientation::Cw));
        assert_eq!(o(false, Orientation::Cw), Some(Orientation::Ccw));
        assert_eq!(o(true, Orientation::Ccw), Some(Orientation::Ccw));
        assert_eq!(o(false, Orientation::Ccw), Some(Orientation::Cw));
    }

    #[test]
    fn validate_rules() {
        let p = Project::new();
        let op = Operation {
            id: 1,
            name: "x".into(),
            enabled: true,
            tool: 1,
            items: vec![],
            kind: OpKind::Pocket { stepover_pct: 50.0 },
            depth: 5.0,
            depth_per_pass: 2.0,
            feed: 800.0,
            plunge_feed: 200.0,
            rpm: 0,
            tabs: Default::default(),
        };
        assert_eq!(validate(&p.machine, &op), Ok(()));
        let err = |f: &dyn Fn(&mut Machine, &mut Operation)| {
            let (mut m, mut o) = (p.machine.clone(), op.clone());
            f(&mut m, &mut o);
            validate(&m, &o).unwrap_err()
        };
        let e = ToolpathError::InvalidParam;
        assert_eq!(err(&|_, o| o.depth = 0.0), e("depth"));
        assert_eq!(err(&|_, o| o.depth = f64::INFINITY), e("depth"));
        assert_eq!(err(&|_, o| o.depth_per_pass = -1.0), e("depth_per_pass"));
        assert_eq!(err(&|_, o| o.depth_per_pass = 1e-4), e("depth_per_pass"));
        assert_eq!(err(&|_, o| o.feed = 0.0), e("feed"));
        assert_eq!(err(&|_, o| o.plunge_feed = f64::NAN), e("plunge_feed"));
        let so = |v| {
            move |_: &mut Machine, o: &mut Operation| o.kind = OpKind::Pocket { stepover_pct: v }
        };
        assert_eq!(err(&so(0.0)), e("stepover_pct"));
        assert_eq!(err(&so(100.5)), e("stepover_pct"));
        assert_eq!(
            err(&|_, o| o.kind = OpKind::Drill { peck: Some(0.0) }),
            e("peck")
        );
        assert_eq!(err(&|m, _| m.clearance_z = 0.0), e("clearance_z"));
        assert_eq!(err(&|m, _| m.safe_z = 1.0), e("safe_z"));
        // drill ignores depth_per_pass and feed
        let mut d = op.clone();
        d.kind = OpKind::Drill { peck: None };
        d.depth_per_pass = 0.0;
        d.feed = f64::NAN;
        assert_eq!(validate(&p.machine, &d), Ok(()));
        let mut full = op.clone();
        full.kind = OpKind::Pocket {
            stepover_pct: 100.0,
        };
        assert_eq!(validate(&p.machine, &full), Ok(()));
    }
}
