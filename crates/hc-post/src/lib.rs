//! G-code output: checked toolpaths → one file per tool segment, in a machine dialect.

mod linuxcnc;

pub use linuxcnc::{sanitize_comment, LinuxCnc};

use hc_geom::Point;
use hc_project::{Project, Units, WorkOrigin};
use hc_toolpath::{check, generate, Issue, Move, Point3, Severity};
use std::fmt;

/// One output file = one tool segment, already checked, in mm, work origin applied. Dialects never see the Project.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub name: String,
    /// File index (1-based) of `count`.
    pub index: usize,
    pub count: usize,
    pub units: Units,
    pub origin: WorkOrigin,
    pub safe_z: f64,
    pub tool_change_z: f64,
    pub tool: u32,
    pub tool_name: String,
    pub ops: Vec<JobOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobOp {
    pub name: String,
    pub rpm: u32,
    pub moves: Vec<Move>,
}

/// File name = `<stem><suffix>.<extension>`.
#[derive(Debug, Clone, PartialEq)]
pub struct GcodeFile {
    pub suffix: String,
    pub text: String,
}

pub trait PostProcessor {
    /// Without dot; LinuxCnc → "ngc".
    fn extension(&self) -> &'static str;
    /// Infallible: `export()` has checked the job.
    fn program(&self, job: &Job) -> String;
}

#[derive(Debug, Clone, PartialEq)]
pub enum PostError {
    UnknownPost(String),
    NothingToExport,
    /// The Error issues of `hc_toolpath::check`, in check order (never empty).
    Blocked(Vec<Issue>),
    /// Non-finite value or first move not a Rapid (internal bug guard); the op id.
    BadToolpath(u64),
}

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostError::UnknownPost(name) => {
                write!(f, "Unknown post processor \"{name}\" (available: linuxcnc)")
            }
            PostError::NothingToExport => write!(f, "Nothing to export: no enabled operations"),
            PostError::Blocked(issues) => {
                let n = issues.len();
                write!(
                    f,
                    "Cannot export: {n} error{}",
                    if n == 1 { "" } else { "s" }
                )?;
                if let Some(i) = issues.first() {
                    // shortcut: the op id, not its name (the error has no Project); the GUI shows names
                    match i.op {
                        Some(op) => write!(f, ". First: Operation {op}: {}", i.message)?,
                        None => write!(f, ". First: Project: {}", i.message)?,
                    }
                }
                Ok(())
            }
            PostError::BadToolpath(id) => {
                write!(f, "Internal error: invalid toolpath for operation {id}")
            }
        }
    }
}

impl std::error::Error for PostError {}

/// Trimmed, ASCII case-insensitive: "linuxcnc" → LinuxCnc; anything else (incl. "") → UnknownPost(original string).
pub fn post_for(name: &str) -> Result<Box<dyn PostProcessor>, PostError> {
    if name.trim().eq_ignore_ascii_case("linuxcnc") {
        Ok(Box::new(LinuxCnc))
    } else {
        Err(PostError::UnknownPost(name.to_string()))
    }
}

/// XY of the work zero in drawing coordinates.
pub fn work_origin(project: &Project) -> Point {
    let s = &project.stock;
    match s.origin {
        WorkOrigin::DrawingOrigin => Point::new(0.0, 0.0),
        WorkOrigin::StockBottomLeft => Point::new(s.min_x, s.min_y),
        WorkOrigin::StockCenter => Point::new(s.min_x + s.width / 2.0, s.min_y + s.height / 2.0),
    }
}

/// Fixed decimals, then trailing zeros and a trailing '.' removed; "-0" → "0". Never exponent notation.
pub fn fmt_num(v: f64, decimals: usize) -> String {
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.truncate(s.trim_end_matches('0').trim_end_matches('.').len());
    }
    if s == "-0" {
        s.remove(0);
    }
    s
}

fn shift(p: Point, o: Point) -> Point {
    Point::new(p.x - o.x, p.y - o.y)
}

fn shift3(p: Point3, o: Point) -> Point3 {
    Point3 {
        x: p.x - o.x,
        y: p.y - o.y,
        z: p.z,
    }
}

/// Every coordinate / feed of the move is finite.
fn finite(m: &Move) -> bool {
    let p3 = |p: &Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    let p2 = |p: &Point| p.x.is_finite() && p.y.is_finite();
    match m {
        Move::Rapid(p) => p3(p),
        Move::Linear { to, feed } => p3(to) && feed.is_finite(),
        Move::Arc {
            to, center, feed, ..
        } => p3(to) && p2(center) && feed.is_finite(),
        Move::Drill {
            at,
            z_bottom,
            peck,
            r_plane,
            feed,
        } => {
            p2(at)
                && z_bottom.is_finite()
                && peck.is_none_or(f64::is_finite)
                && r_plane.is_finite()
                && feed.is_finite()
        }
    }
}

fn shift_move(m: Move, o: Point) -> Move {
    match m {
        Move::Rapid(p) => Move::Rapid(shift3(p, o)),
        Move::Linear { to, feed } => Move::Linear {
            to: shift3(to, o),
            feed,
        },
        Move::Arc {
            to,
            center,
            ccw,
            feed,
        } => Move::Arc {
            to: shift3(to, o),
            center: shift(center, o),
            ccw,
            feed,
        },
        Move::Drill {
            at,
            z_bottom,
            peck,
            r_plane,
            feed,
        } => Move::Drill {
            at: shift(at, o),
            z_bottom,
            peck,
            r_plane,
            feed,
        },
    }
}

/// Files in machining order. `name` (the project stem) goes into the headers.
pub fn export(project: &Project, name: &str) -> Result<Vec<GcodeFile>, PostError> {
    let post = post_for(&project.machine.post)?;
    let m = &project.machine;
    let paths = generate(project);
    if paths.is_empty() {
        return Err(PostError::NothingToExport);
    }
    let errors: Vec<Issue> = check(project, &paths)
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        return Err(PostError::Blocked(errors));
    }
    let origin = work_origin(project);
    let op_name = |id: u64| {
        project
            .operations
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.name.clone())
            .unwrap_or_default()
    };
    // (tool, ops) per segment
    let mut segments: Vec<(u32, Vec<JobOp>)> = Vec::new();
    for (id, res) in paths {
        // check() reports every Err, so this is the internal guard only
        let Ok(tp) = res else {
            return Err(PostError::BadToolpath(id));
        };
        let moves: Vec<Move> = tp
            .moves
            .into_iter()
            .map(|mv| shift_move(mv, origin))
            .collect();
        // checked after the shift so a non-finite stock origin is caught too
        if !matches!(moves.first(), Some(Move::Rapid(_))) || !moves.iter().all(finite) {
            return Err(PostError::BadToolpath(id));
        }
        let op = JobOp {
            name: op_name(id),
            rpm: tp.rpm,
            moves,
        };
        match segments.last_mut() {
            Some((tool, ops)) if *tool == tp.tool_id => ops.push(op),
            _ => segments.push((tp.tool_id, vec![op])),
        }
    }
    let count = segments.len();
    let width = count.to_string().len();
    Ok(segments
        .into_iter()
        .enumerate()
        .map(|(k, (tool, ops))| {
            let job = Job {
                name: name.to_string(),
                index: k + 1,
                count,
                units: project.units,
                origin: project.stock.origin,
                safe_z: m.safe_z,
                tool_change_z: m.tool_change_z,
                tool,
                tool_name: project
                    .tools
                    .iter()
                    .find(|t| t.id == tool)
                    .map(|t| t.name.clone())
                    .unwrap_or_default(),
                ops,
            };
            let suffix = if count == 1 {
                String::new()
            } else {
                format!("_{:0width$}_T{tool}", k + 1)
            };
            GcodeFile {
                suffix,
                text: post.program(&job),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_rules() {
        assert_eq!(fmt_num(-0.0004, 3), "0");
        assert_eq!(fmt_num(1e-12, 3), "0");
        assert_eq!(fmt_num(-0.0, 3), "0");
        assert_eq!(fmt_num(10.0, 3), "10");
        assert_eq!(fmt_num(-1.5, 3), "-1.5");
        assert_eq!(fmt_num(0.1 + 0.2, 3), "0.3");
        assert_eq!(fmt_num(1e15, 3), "1000000000000000");
        assert_eq!(fmt_num(0.00049, 4), "0.0005");
        assert_eq!(fmt_num(25.4 / 25.4, 4), "1");
        // ties: Rust's formatter on the exact binary value, half to even (M7 review ruling 2a)
        assert_eq!(fmt_num(0.0625, 3), "0.062");
        assert_eq!(fmt_num(0.1875, 3), "0.188");
        assert_eq!(fmt_num(100.0, 0), "100");
        assert_eq!(fmt_num(-0.4, 0), "0");
    }

    #[test]
    fn post_for_names() {
        assert!(post_for("linuxcnc").is_ok());
        assert!(post_for(" LinuxCNC ").is_ok());
        assert_eq!(
            post_for("").err(),
            Some(PostError::UnknownPost(String::new()))
        );
        assert_eq!(
            post_for("grbl").err().unwrap().to_string(),
            "Unknown post processor \"grbl\" (available: linuxcnc)"
        );
    }

    #[test]
    fn blocked_display() {
        let i = |op| Issue {
            severity: Severity::Error,
            rule: hc_toolpath::RuleId::NoItems,
            op,
            item: None,
            message: "No geometry assigned".into(),
        };
        assert_eq!(
            PostError::Blocked(vec![i(Some(3)), i(None)]).to_string(),
            "Cannot export: 2 errors. First: Operation 3: No geometry assigned"
        );
        assert_eq!(
            PostError::Blocked(vec![i(None)]).to_string(),
            "Cannot export: 1 error. First: Project: No geometry assigned"
        );
    }

    #[test]
    fn finite_checks_every_field() {
        let p = Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(finite(&Move::Rapid(p)));
        assert!(!finite(&Move::Linear {
            to: p,
            feed: f64::NAN
        }));
        let drill = |peck| Move::Drill {
            at: Point::new(0.0, 0.0),
            z_bottom: -1.0,
            peck,
            r_plane: 2.0,
            feed: 100.0,
        };
        assert!(finite(&drill(None)));
        assert!(!finite(&drill(Some(f64::INFINITY))));
    }
}
