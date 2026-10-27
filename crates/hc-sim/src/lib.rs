//! Simulation: toolpaths → a timeline of steps in export order, position at time t, time estimate.
//! Units mm, mm/min and seconds. No acceleration, tool changes not timed (see specs/M9.md).

use hc_geom::Point;
use hc_toolpath::{Move, Point3, Toolpath, ToolpathError};
use std::f64::consts::TAU;
use std::fmt;

/// mm/min; see the M9 Decisions.
pub const DEFAULT_RAPID: f64 = 2500.0;
/// Seconds; the same as hc-post's `G4 P2`.
pub const SPINUP_S: f64 = 2.0;
pub const MAX_PECKS: usize = 10_000;

/// Moves shorter than this (mm, 3D) produce no step.
const ZERO_LEN: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Rapid,
    Feed,
    Dwell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: StepKind,
    pub from: Point3,
    pub to: Point3,
    /// XY arc: centre and signed sweep in rad (+ = ccw, |sweep| <= π). None = straight.
    pub arc: Option<(Point, f64)>,
    /// mm/min: the rapid rate for Rapid, the move feed for Feed, 0 for Dwell.
    pub feed: f64,
    /// Seconds from the start of the whole job.
    pub t0: f64,
    pub t1: f64,
    pub op: u64,
    pub tool: u32,
    pub rpm: u32,
}

impl Step {
    /// Position at absolute time t, clamped to [t0, t1]. Line: lerp. Arc: angle, radius and Z all linear in time.
    pub fn point_at(&self, t: f64) -> Point3 {
        if t >= self.t1 {
            return self.to;
        }
        if t <= self.t0 {
            return self.from;
        }
        let f = (t - self.t0) / (self.t1 - self.t0);
        let z = self.from.z + (self.to.z - self.from.z) * f;
        match self.arc {
            None => Point3 {
                x: self.from.x + (self.to.x - self.from.x) * f,
                y: self.from.y + (self.to.y - self.from.y) * f,
                z,
            },
            Some((c, sweep)) => {
                let d0 = self.from.xy() - c;
                let r0 = d0.length();
                let r = r0 + ((self.to.xy() - c).length() - r0) * f;
                let a = d0.y.atan2(d0.x) + sweep * f;
                Point3 {
                    x: c.x + r * a.cos(),
                    y: c.y + r * a.sin(),
                    z,
                }
            }
        }
    }
}

/// One G-code file.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub tool: u32,
    pub ops: Vec<u64>,
    pub t0: f64,
    pub t1: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sim {
    pub steps: Vec<Step>,
    pub blocks: Vec<Block>,
    /// Ops whose toolpath is an Err (not simulated), in order.
    pub skipped: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimError {
    /// The op id and the index of the move in `Toolpath::moves`.
    BadMove { op: u64, index: usize },
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::BadMove { op, index } => {
                write!(f, "Cannot simulate operation {op}: move {index} is invalid")
            }
        }
    }
}

impl std::error::Error for SimError {}

fn finite3(p: &Point3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

fn good_feed(f: f64) -> bool {
    f.is_finite() && f > 0.0
}

/// Peck count n = ceil((r − bottom)/q − 1e-9), at least 0; None if it would exceed MAX_PECKS.
fn peck_count(r_plane: f64, z_bottom: f64, q: f64) -> Option<usize> {
    let n = ((r_plane - z_bottom) / q - 1e-9).ceil().max(0.0);
    (n <= MAX_PECKS as f64).then_some(n as usize)
}

fn valid(m: &Move) -> bool {
    match m {
        Move::Rapid(p) => finite3(p),
        Move::Linear { to, feed } => finite3(to) && good_feed(*feed),
        Move::Arc {
            to, center, feed, ..
        } => finite3(to) && center.x.is_finite() && center.y.is_finite() && good_feed(*feed),
        Move::Drill {
            at,
            z_bottom,
            peck,
            r_plane,
            feed,
        } => {
            at.x.is_finite()
                && at.y.is_finite()
                && z_bottom.is_finite()
                && r_plane.is_finite()
                && good_feed(*feed)
                && peck.is_none_or(|q| good_feed(q) && peck_count(*r_plane, *z_bottom, q).is_some())
        }
    }
}

/// Appends steps for one op; `pos` is the current position.
struct Builder<'a> {
    steps: &'a mut Vec<Step>,
    clock: f64,
    pos: Point3,
    rapid: f64,
    op: u64,
    tool: u32,
    rpm: u32,
}

impl Builder<'_> {
    fn push(&mut self, kind: StepKind, to: Point3, arc: Option<(Point, f64)>, feed: f64, len: f64) {
        let from = self.pos;
        self.pos = to;
        if len < ZERO_LEN {
            return;
        }
        let t0 = self.clock;
        self.clock += len / feed * 60.0;
        self.steps.push(Step {
            kind,
            from,
            to,
            arc,
            feed,
            t0,
            t1: self.clock,
            op: self.op,
            tool: self.tool,
            rpm: self.rpm,
        });
    }

    fn line(&mut self, kind: StepKind, to: Point3, feed: f64) {
        let p = self.pos;
        let len = (to.x - p.x).hypot(to.y - p.y).hypot(to.z - p.z);
        self.push(kind, to, None, feed, len);
    }

    fn rapid(&mut self, to: Point3) {
        self.line(StepKind::Rapid, to, self.rapid);
    }

    fn rapid_z(&mut self, z: f64) {
        self.rapid(Point3 { z, ..self.pos });
    }

    fn feed_z(&mut self, z: f64, feed: f64) {
        self.line(StepKind::Feed, Point3 { z, ..self.pos }, feed);
    }

    fn dwell(&mut self) {
        let t0 = self.clock;
        self.clock += SPINUP_S;
        self.steps.push(Step {
            kind: StepKind::Dwell,
            from: self.pos,
            to: self.pos,
            arc: None,
            feed: 0.0,
            t0,
            t1: self.clock,
            op: self.op,
            tool: self.tool,
            rpm: self.rpm,
        });
    }

    fn mv(&mut self, m: &Move) {
        match *m {
            Move::Rapid(q) => self.rapid(q),
            Move::Linear { to, feed } => self.line(StepKind::Feed, to, feed),
            Move::Arc {
                to,
                center,
                ccw,
                feed,
            } => {
                let d = self.pos.xy() - center;
                let a0 = d.y.atan2(d.x);
                let a1 = (to.y - center.y).atan2(to.x - center.x);
                let sweep = if ccw {
                    (a1 - a0).rem_euclid(TAU)
                } else {
                    -(a0 - a1).rem_euclid(TAU)
                };
                if sweep.abs() < 1e-12 {
                    // the post writes a zero-sweep arc as G1 (M7 ruling 1)
                    self.line(StepKind::Feed, to, feed);
                } else {
                    let len = (d.length() * sweep.abs()).hypot(to.z - self.pos.z);
                    self.push(StepKind::Feed, to, Some((center, sweep)), feed, len);
                }
            }
            Move::Drill {
                at,
                z_bottom,
                peck,
                r_plane,
                feed,
            } => {
                let z0 = self.pos.z;
                // LinuxCNC: below R, the cycle first rapids up to R, then moves in XY
                if z0 < r_plane {
                    self.rapid_z(r_plane);
                }
                self.rapid(Point3 {
                    x: at.x,
                    y: at.y,
                    z: self.pos.z,
                });
                self.rapid_z(r_plane);
                match peck {
                    None => self.feed_z(z_bottom, feed),
                    Some(q) => {
                        // `valid` checked the count
                        let n = peck_count(r_plane, z_bottom, q).unwrap_or(0);
                        let b = |k: usize| (r_plane - k as f64 * q).max(z_bottom);
                        for k in 1..=n {
                            if k > 1 {
                                self.rapid_z(b(k - 1));
                            }
                            self.feed_z(b(k), feed);
                            if k < n {
                                self.rapid_z(r_plane);
                            }
                        }
                    }
                }
                self.rapid_z(z0.max(r_plane));
            }
        }
    }
}

/// `toolpaths` = the output of `hc_toolpath::generate`. Pure and deterministic.
pub fn simulate(
    toolpaths: &[(u64, Result<Toolpath, ToolpathError>)],
    rapid: f64,
) -> Result<Sim, SimError> {
    assert!(rapid.is_finite() && rapid > 0.0, "bad rapid rate {rapid}");
    let mut sim = Sim::default();
    let mut clock = 0.0;
    let mut pos = Point3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut prev_rpm = 0;
    for (id, res) in toolpaths {
        let Ok(tp) = res else {
            sim.skipped.push(*id);
            continue;
        };
        if let Some(index) = tp.moves.iter().position(|m| !valid(m)) {
            return Err(SimError::BadMove { op: *id, index });
        }
        let Some(Move::Rapid(start)) = tp.moves.first() else {
            return Err(SimError::BadMove { op: *id, index: 0 });
        };
        // same grouping as hc_post::export: a new file whenever the tool changes
        let new_block = sim.blocks.last().is_none_or(|b| b.tool != tp.tool_id);
        if new_block {
            sim.blocks.push(Block {
                tool: tp.tool_id,
                ops: Vec::new(),
                t0: clock,
                t1: clock,
            });
            pos = *start; // manual tool change: the real start position is unknown
        }
        let mut b = Builder {
            steps: &mut sim.steps,
            clock,
            pos,
            rapid,
            op: *id,
            tool: tp.tool_id,
            rpm: tp.rpm,
        };
        if new_block || tp.rpm != prev_rpm {
            b.dwell();
        }
        for (index, m) in tp.moves.iter().enumerate() {
            b.mv(m);
            // finite but extreme values can still overflow the clock
            if !b.clock.is_finite() {
                return Err(SimError::BadMove { op: *id, index });
            }
        }
        (clock, pos, prev_rpm) = (b.clock, b.pos, tp.rpm);
        if let Some(blk) = sim.blocks.last_mut() {
            blk.ops.push(*id);
            blk.t1 = clock;
        }
    }
    Ok(sim)
}

impl Sim {
    /// `blocks.last().t1`, or 0.
    pub fn total(&self) -> f64 {
        self.blocks.last().map_or(0.0, |b| b.t1)
    }

    /// The step active at t: the first step with t1 >= t, clamped to the last step; t <= 0 → 0. None without steps.
    pub fn step_at(&self, t: f64) -> Option<usize> {
        if self.steps.is_empty() {
            return None;
        }
        if t <= 0.0 {
            return Some(0);
        }
        let i = self.steps.partition_point(|s| s.t1 < t);
        Some(i.min(self.steps.len() - 1))
    }

    pub fn position_at(&self, t: f64) -> Option<Point3> {
        self.step_at(t).map(|i| self.steps[i].point_at(t))
    }
}

/// Rounded to whole seconds: 4.6 → "0:05", 125 → "2:05", 3725 → "1:02:05". Negative / NaN → "0:00".
pub fn fmt_duration(secs: f64) -> String {
    let s = if secs > 0.0 { secs.round() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
