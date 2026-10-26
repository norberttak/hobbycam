//! dxf entities → hc-geom geometry in mm (OCS mirror, INSERT explode, layer collection).

use crate::flatten::{arc_points, ellipse_points, spline_points};
use crate::{DxfImport, ImportOptions, ImportWarning, Layer};
use dxf::entities::{Entity, EntityType};
use dxf::{Drawing, Vector};
use hc_geom::{chain_segments, Circle, Contour, Point, Segment, Transform, EPS};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// Entity types handled here (plus sub-entities); everything else is reported as ignored.
pub const SUPPORTED: &[&str] = &[
    "LINE",
    "ARC",
    "CIRCLE",
    "LWPOLYLINE",
    "POLYLINE",
    "SPLINE",
    "ELLIPSE",
    "POINT",
    "INSERT",
    "VERTEX",
    "SEQEND",
    "ATTRIB",
];

const MAX_INSERT_DEPTH: usize = 16;

/// Geometry of one entity in its local (block/OCS) coordinates.
enum Geo {
    Closed(Contour),
    Open(Vec<Segment>),
    Pt(Point),
}

#[derive(Default)]
struct LayerAcc {
    name: String,
    closed: Vec<Contour>,
    pool: Vec<Segment>,
    points: Vec<Point>,
}

struct Importer<'a> {
    drawing: &'a Drawing,
    opts: &'a ImportOptions,
    unit: Transform,
    layers: Vec<LayerAcc>,
    warnings: Vec<ImportWarning>,
    ignored: BTreeMap<String, usize>,
    /// names of the blocks currently being expanded (cycle guard)
    stack: Vec<String>,
}

fn p2(p: &dxf::Point) -> Point {
    Point::new(p.x, p.y)
}

fn unit_scale(units: dxf::enums::Units) -> Option<f64> {
    use dxf::enums::Units::*;
    match units {
        Inches => Some(25.4),
        Feet => Some(304.8),
        Millimeters => Some(1.0),
        Centimeters => Some(10.0),
        Meters => Some(1000.0),
        _ => None,
    }
}

/// Lengths of the two linear columns and whether the transform keeps circles circular.
fn conformal(t: &Transform) -> (bool, f64) {
    let l1 = t.a.hypot(t.b);
    let l2 = t.c.hypot(t.d);
    let max = l1.max(l2);
    let ok = (l1 - l2).abs() <= 1e-9 * max && (t.a * t.c + t.b * t.d).abs() <= 1e-9 * l1 * l2;
    (ok, max)
}

fn polyline(pts: &[Point], closed: bool) -> Contour {
    let v: Vec<(Point, f64)> = pts.iter().map(|&p| (p, 0.0)).collect();
    Contour::from_bulge_vertices(&v, closed)
}

pub fn import_drawing(drawing: &Drawing, opts: &ImportOptions) -> DxfImport {
    let units = drawing.header.default_drawing_units;
    let mut warnings = Vec::new();
    let scale = unit_scale(units).unwrap_or_else(|| {
        warnings.push(ImportWarning::UnitsAssumedMm {
            insunits: units as i16,
        });
        1.0
    });
    let mut imp = Importer {
        drawing,
        opts,
        unit: Transform::scale(scale, Point::new(0.0, 0.0)),
        layers: Vec::new(),
        warnings,
        ignored: BTreeMap::new(),
        stack: Vec::new(),
    };

    let mut paper = 0;
    for e in drawing.entities() {
        if e.common.is_in_paper_space {
            paper += 1;
        } else {
            imp.entity(e, &Transform::identity(), None);
        }
    }
    if paper > 0 {
        imp.warnings
            .push(ImportWarning::PaperSpaceSkipped { count: paper });
    }

    let chain_tol = opts.chain_tol_mm;
    let layers = imp
        .layers
        .into_iter()
        .map(|acc| {
            let color = drawing
                .layers()
                .find(|l| l.name == acc.name)
                .and_then(|l| l.color.index())
                .map_or(7, i16::from);
            let mut contours = acc.closed;
            contours.extend(chain_segments(&acc.pool, chain_tol));
            Layer {
                name: acc.name,
                color,
                contours,
                points: acc.points,
            }
        })
        .collect();
    DxfImport {
        layers,
        unit_scale: scale,
        ignored: imp.ignored,
        warnings: imp.warnings,
    }
}

impl Importer<'_> {
    /// OCS handling: identity for +Z, mirror for −Z, None (+warning) for any tilted normal.
    fn ocs(&mut self, n: &Vector, name: &str) -> Option<Transform> {
        if n.x.abs() > 1e-9 || n.y.abs() > 1e-9 {
            self.warnings.push(ImportWarning::UnsupportedExtrusion {
                entity: name.to_string(),
            });
            None
        } else if n.z < 0.0 {
            Some(Transform::mirror_y(0.0))
        } else {
            Some(Transform::identity())
        }
    }

    fn entity(&mut self, e: &Entity, parent: &Transform, parent_layer: Option<&str>) {
        let layer = match parent_layer {
            Some(pl) if e.common.layer == "0" => pl.to_string(),
            _ => e.common.layer.clone(),
        };
        // OCS transform of the entity (identity for WCS entities)
        let ocs = match &e.specific {
            EntityType::Arc(a) => self.ocs(&a.normal, "ARC"),
            EntityType::Circle(c) => self.ocs(&c.normal, "CIRCLE"),
            EntityType::LwPolyline(p) => self.ocs(&p.extrusion_direction, "LWPOLYLINE"),
            EntityType::Polyline(p) => self.ocs(&p.normal, "POLYLINE"),
            EntityType::Ellipse(el) => self
                .ocs(&el.normal, "ELLIPSE")
                .map(|_| Transform::identity()),
            EntityType::Insert(i) => self.ocs(&i.extrusion_direction, "INSERT"),
            _ => Some(Transform::identity()),
        };
        let Some(ocs) = ocs else { return };

        if let EntityType::Insert(ins) = &e.specific {
            self.insert(ins, &ocs.then(parent), &layer);
            return;
        }

        let world = ocs.then(parent).then(&self.unit);
        let (conf, max_scale) = conformal(&world);
        // flatten tolerance in the entity's own units
        let tol = self.opts.flatten_tol_mm / max_scale.max(EPS);
        let geo = match &e.specific {
            EntityType::Line(l) => Some(Geo::Open(vec![Segment::Line {
                start: p2(&l.p1),
                end: p2(&l.p2),
            }])),
            EntityType::ModelPoint(p) => Some(Geo::Pt(p2(&p.location))),
            EntityType::Circle(c) if c.radius >= EPS => Some(Geo::Closed(
                Circle {
                    center: p2(&c.center),
                    radius: c.radius,
                }
                .to_contour(),
            )),
            EntityType::Arc(a) if a.radius >= EPS => {
                let mut sweep = (a.end_angle - a.start_angle).rem_euclid(360.0).to_radians();
                if sweep < EPS {
                    sweep = TAU;
                }
                Some(Geo::Open(vec![Segment::Arc {
                    center: p2(&a.center),
                    radius: a.radius,
                    start_angle: a.start_angle.to_radians(),
                    sweep,
                }]))
            }
            EntityType::LwPolyline(p) => {
                let v: Vec<(Point, f64)> = p
                    .vertices
                    .iter()
                    .map(|v| (Point::new(v.x, v.y), v.bulge))
                    .collect();
                Some(bulge_geo(&v, p.flags & 1 != 0))
            }
            EntityType::Polyline(p) => {
                if p.flags & (8 | 16 | 64) != 0 {
                    *self
                        .ignored
                        .entry("POLYLINE (3D/mesh)".to_string())
                        .or_insert(0) += 1;
                    return;
                }
                let v: Vec<(Point, f64)> = p
                    .vertices()
                    .filter(|v| v.flags & 16 == 0)
                    .map(|v| (p2(&v.location), v.bulge))
                    .collect();
                Some(bulge_geo(&v, p.flags & 1 != 0))
            }
            EntityType::Ellipse(el) => ellipse_geo(el, tol),
            EntityType::Spline(s) => self.spline_geo(s, &layer, tol),
            _ => None,
        };
        if let Some(g) = geo {
            self.emit(&layer, g, &world, conf, tol);
        }
    }

    fn insert(&mut self, ins: &dxf::entities::Insert, parent: &Transform, layer: &str) {
        if ins.name.starts_with("*D") {
            return;
        }
        let Some(block) = self.drawing.blocks().find(|b| b.name == ins.name) else {
            self.warnings.push(ImportWarning::MissingBlock {
                name: ins.name.clone(),
            });
            return;
        };
        if self.stack.len() >= MAX_INSERT_DEPTH || self.stack.contains(&ins.name) {
            let w = ImportWarning::InsertNestingTooDeep {
                block: ins.name.clone(),
            };
            if !self.warnings.contains(&w) {
                self.warnings.push(w);
            }
            return;
        }
        if ins.row_count > 1 || ins.column_count > 1 {
            self.warnings.push(ImportWarning::InsertArrayFirstOnly {
                block: ins.name.clone(),
            });
        }
        let (sx, sy) = (ins.x_scale_factor, ins.y_scale_factor);
        let base = p2(&block.base_point);
        // translate(-base) then scale(sx, sy)
        let local = Transform {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: -base.x * sx,
            ty: -base.y * sy,
        }
        .then(&Transform::rotate(
            ins.rotation.to_radians(),
            Point::new(0.0, 0.0),
        ))
        .then(&Transform::translate(ins.location.x, ins.location.y))
        .then(parent);
        self.stack.push(ins.name.clone());
        for e in &block.entities {
            self.entity(e, &local, Some(layer));
        }
        self.stack.pop();
    }

    /// None (counted as "SPLINE (invalid)") when the spline cannot be built.
    fn spline_geo(&mut self, s: &dxf::entities::Spline, layer: &str, tol: f64) -> Option<Geo> {
        let pts = if !s.control_points.is_empty() {
            let ctrl: Vec<Point> = s.control_points.iter().map(p2).collect();
            let degree = usize::try_from(s.degree_of_curve).unwrap_or(0);
            spline_points(degree, &ctrl, &s.knot_values, &s.weight_values, tol)
        } else if s.fit_points.len() >= 2 {
            // shortcut: fit-point interpolation is not implemented; upgrade if real files need it
            self.warnings.push(ImportWarning::SplineFitPointsOnly {
                layer: layer.to_string(),
            });
            Some(s.fit_points.iter().map(p2).collect())
        } else {
            None
        };
        let Some(pts) = pts else {
            *self
                .ignored
                .entry("SPLINE (invalid)".to_string())
                .or_insert(0) += 1;
            return None;
        };
        let ends_meet = pts[0].approx_eq(pts[pts.len() - 1], tol);
        if s.flags & 1 != 0 || ends_meet {
            let n = if ends_meet { pts.len() - 1 } else { pts.len() };
            Some(Geo::Closed(polyline(&pts[..n], true)))
        } else {
            Some(Geo::Open(polyline(&pts, false).segments))
        }
    }

    /// Transform to mm and store; arcs are flattened first when the transform is not conformal.
    fn emit(&mut self, layer: &str, g: Geo, world: &Transform, conformal: bool, tol: f64) {
        let fix = |segs: Vec<Segment>| -> Vec<Segment> {
            let mut out = Vec::with_capacity(segs.len());
            for s in segs {
                match s {
                    Segment::Arc {
                        center,
                        radius,
                        start_angle,
                        sweep,
                    } if !conformal => {
                        let pts = arc_points(center, radius, start_angle, sweep, tol);
                        out.extend(
                            polyline(&pts, false)
                                .segments
                                .iter()
                                .map(|l| l.transformed(world)),
                        );
                    }
                    _ => out.push(s.transformed(world)),
                }
            }
            out.retain(|s| s.length() >= EPS);
            out
        };
        let idx = match self.layers.iter().position(|l| l.name == layer) {
            Some(i) => i,
            None => {
                self.layers.push(LayerAcc {
                    name: layer.to_string(),
                    ..Default::default()
                });
                self.layers.len() - 1
            }
        };
        match g {
            Geo::Pt(p) => self.layers[idx].points.push(world.apply(p)),
            Geo::Open(segs) => {
                let segs = fix(segs);
                self.layers[idx].pool.extend(segs);
            }
            Geo::Closed(c) => {
                let segments = fix(c.segments);
                if !segments.is_empty() {
                    self.layers[idx].closed.push(Contour {
                        segments,
                        closed: true,
                    });
                }
            }
        }
    }
}

fn bulge_geo(v: &[(Point, f64)], closed: bool) -> Geo {
    let c = Contour::from_bulge_vertices(v, closed);
    if closed {
        Geo::Closed(c)
    } else {
        Geo::Open(c.segments)
    }
}

fn ellipse_geo(el: &dxf::entities::Ellipse, tol: f64) -> Option<Geo> {
    let c = p2(&el.center);
    let major = Point::new(el.major_axis.x, el.major_axis.y);
    let nz = if el.normal.z < 0.0 { -1.0 } else { 1.0 };
    let ratio = el.minor_axis_ratio;
    let minor = Point::new(-nz * major.y, nz * major.x) * ratio;
    let t0 = el.start_parameter;
    let mut dt = el.end_parameter - t0;
    if dt < 0.0 {
        dt += TAU;
    }
    let full = dt < EPS || (dt - TAU).abs() < 1e-6;
    if full {
        dt = TAU;
    }
    if major.length() < EPS {
        return None;
    }
    if (ratio - 1.0).abs() < 1e-9 {
        let arc = Segment::Arc {
            center: c,
            radius: major.length(),
            start_angle: major.y.atan2(major.x) + nz * t0,
            sweep: nz * dt,
        };
        return Some(if full {
            Geo::Closed(Contour {
                segments: vec![arc],
                closed: true,
            })
        } else {
            Geo::Open(vec![arc])
        });
    }
    let pts = ellipse_points(c, major, minor, t0, dt, tol);
    if full {
        Some(Geo::Closed(polyline(&pts[..pts.len() - 1], true)))
    } else {
        Some(Geo::Open(polyline(&pts, false).segments))
    }
}
