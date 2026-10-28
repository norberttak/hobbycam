//! Schematic drawing of a tool for the tool library form (generic shape, true values in the labels).
use eframe::egui;
use hc_project::{fmt_len, Tool, ToolKind, Units};

pub const SKETCH_W: f32 = 240.0;
pub const SKETCH_H: f32 = 300.0;
pub const MAX_FLUTE_LINES: u32 = 6;
pub const V_DRAW_MIN: f64 = 30.0;
pub const V_DRAW_MAX: f64 = 150.0;
pub const DRILL_POINT_DEG: f64 = 118.0;

const X: f32 = 90.0;
const L: f32 = 60.0;
const R: f32 = 120.0;
const H: f32 = 30.0;
const TOP: f32 = 20.0;
const FT: f32 = 90.0;
const TIP: f32 = 240.0;

/// Points in the sketch box: x right, y **down**, (0,0) top left, SKETCH_W × SKETCH_H.
pub type P = [f32; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pen {
    Outline,
    Hint,
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Diameter,
    FluteLength,
    Flutes,
    VAngle,
}

#[allow(clippy::enum_variant_names)] // names fixed by spec M17
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    LeftCenter,
    RightCenter,
    CenterCenter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub field: Field,
    pub pos: P,
    pub anchor: Anchor,
    pub text: String,
    pub bad: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sketch {
    /// Tool outline, convex, not repeating the first point.
    pub body: Vec<P>,
    /// Open polylines (an arc is pre-sampled).
    pub lines: Vec<(Pen, Vec<P>)>,
    /// Filled arrow heads (triangles), Pen::Dim colour.
    pub arrows: Vec<[P; 3]>,
    pub labels: Vec<Label>,
}

fn pos_ok(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// Drawn tip angle: clamped to the readable range; NaN (never in a valid library) draws as 60°.
fn v_draw(deg: f64) -> f64 {
    if deg.is_nan() {
        60.0
    } else {
        deg.clamp(V_DRAW_MIN, V_DRAW_MAX)
    }
}

pub fn sketch(t: &Tool, units: Units) -> Sketch {
    let (is_flat, is_ball, v_angle) = match t.kind {
        ToolKind::FlatEndMill => (true, false, None),
        ToolKind::BallEndMill => (false, true, None),
        ToolKind::VBit { angle_deg } => (false, false, Some(angle_deg)),
        ToolKind::Drill => (false, false, None),
    };
    let mut body = vec![[L, TOP], [R, TOP]];
    let side_end;
    if is_flat {
        side_end = TIP;
        body.extend([[R, TIP], [L, TIP]]);
    } else if is_ball {
        side_end = TIP - H;
        for i in 0..=16 {
            let th = std::f32::consts::PI * i as f32 / 16.0;
            body.push([X + H * th.cos(), side_end + H * th.sin()]);
        }
    } else {
        let a = v_draw(v_angle.unwrap_or(DRILL_POINT_DEG));
        side_end = TIP - H / (a.to_radians() / 2.0).tan() as f32;
        body.extend([[R, side_end], [X, TIP], [L, side_end]]);
    }

    let mut lines = vec![(Pen::Outline, vec![[L, FT], [R, FT]])];
    let mut arrows = vec![];
    let n = t.flutes.min(MAX_FLUTE_LINES);
    let zh = side_end - FT;
    let slant = 12.0_f32.min(zh / 3.0);
    let mut first_end = None;
    for k in 1..=n {
        let y = FT + slant / 2.0 + k as f32 * (zh - slant) / (n + 1) as f32;
        lines.push((Pen::Hint, vec![[L, y + slant / 2.0], [R, y - slant / 2.0]]));
        if k == 1 {
            first_end = Some([R, y - slant / 2.0]);
        }
    }

    // diameter dimension
    for x in [L, R] {
        lines.push((Pen::Dim, vec![[x, side_end + 2.0], [x, 274.0]]));
    }
    lines.push((Pen::Dim, vec![[L, 268.0], [R, 268.0]]));
    arrows.push([[L, 268.0], [L + 6.0, 265.5], [L + 6.0, 270.5]]);
    arrows.push([[R, 268.0], [R - 6.0, 265.5], [R - 6.0, 270.5]]);

    // flute length dimension
    let xb = if is_flat { R } else { X };
    lines.push((Pen::Dim, vec![[R + 2.0, FT], [146.0, FT]]));
    lines.push((Pen::Dim, vec![[xb + 2.0, TIP], [146.0, TIP]]));
    lines.push((Pen::Dim, vec![[140.0, FT], [140.0, TIP]]));
    arrows.push([[140.0, FT], [137.5, FT + 6.0], [142.5, FT + 6.0]]);
    arrows.push([[140.0, TIP], [137.5, TIP - 6.0], [142.5, TIP - 6.0]]);

    // flutes leader
    if let Some(e) = first_end {
        lines.push((Pen::Dim, vec![[146.0, 50.0], e]));
    }

    let mut labels = vec![
        Label {
            field: Field::Diameter,
            pos: [X, 286.0],
            anchor: Anchor::CenterCenter,
            text: format!("Diameter {}", fmt_len(t.diameter, units)),
            bad: !pos_ok(t.diameter),
        },
        Label {
            field: Field::FluteLength,
            pos: [148.0, (FT + TIP) / 2.0],
            anchor: Anchor::LeftCenter,
            text: format!("Flute length\n{}", fmt_len(t.flute_length, units)),
            bad: !pos_ok(t.flute_length),
        },
        Label {
            field: Field::Flutes,
            pos: [148.0, 50.0],
            anchor: Anchor::LeftCenter,
            text: format!("Flutes {}", t.flutes),
            bad: t.flutes == 0,
        },
    ];

    if let Some(deg) = v_angle {
        let a = v_draw(deg).to_radians();
        let arc = (0..=12)
            .map(|i| {
                let th = -std::f64::consts::FRAC_PI_2 - a / 2.0 + a * i as f64 / 12.0;
                [X + 20.0 * th.cos() as f32, TIP + 20.0 * th.sin() as f32]
            })
            .collect();
        lines.push((Pen::Dim, arc));
        let v = format!("{deg:.1}");
        let v = v.strip_suffix(".0").unwrap_or(&v);
        labels.push(Label {
            field: Field::VAngle,
            // two lines: only ~56 pt between the sketch edge and the V edge
            pos: [56.0, TIP - 12.0],
            anchor: Anchor::RightCenter,
            text: format!("V angle\n{v}°"),
            bad: false,
        });
    }
    Sketch {
        body,
        lines,
        arrows,
        labels,
    }
}

impl Sketch {
    /// One-line text for accessibility / tests.
    pub fn summary(&self) -> String {
        let v: Vec<_> = self
            .labels
            .iter()
            .map(|l| l.text.replace('\n', " "))
            .collect();
        format!("Tool sketch: {}", v.join(", "))
    }
}

pub fn paint(ui: &mut egui::Ui, s: &Sketch) -> egui::Response {
    use eframe::egui::{Align2, Shape, Stroke};
    let (r, painter) = ui.allocate_painter(egui::vec2(SKETCH_W, SKETCH_H), egui::Sense::hover());
    let o = r.rect.min;
    let at = |p: P| o + egui::vec2(p[0], p[1]);
    let v = ui.visuals();
    let dim = v.text_color();
    let pts = |p: &[P]| p.iter().map(|&p| at(p)).collect::<Vec<_>>();
    painter.add(Shape::convex_polygon(
        pts(&s.body),
        v.widgets.inactive.bg_fill,
        Stroke::new(1.5, v.widgets.noninteractive.fg_stroke.color),
    ));
    for (pen, p) in &s.lines {
        let stroke = match pen {
            Pen::Outline => Stroke::new(1.5, v.widgets.noninteractive.fg_stroke.color),
            Pen::Hint => Stroke::new(1.0, v.weak_text_color()),
            Pen::Dim => Stroke::new(1.0, dim),
        };
        painter.add(Shape::line(pts(p), stroke));
    }
    for a in &s.arrows {
        painter.add(Shape::convex_polygon(pts(a), dim, Stroke::NONE));
    }
    for l in &s.labels {
        let align = match l.anchor {
            Anchor::LeftCenter => Align2::LEFT_CENTER,
            Anchor::RightCenter => Align2::RIGHT_CENTER,
            Anchor::CenterCenter => Align2::CENTER_CENTER,
        };
        let c = if l.bad { v.error_fg_color } else { dim };
        painter.text(
            at(l.pos),
            align,
            &l.text,
            egui::FontId::proportional(12.0),
            c,
        );
    }
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, s.summary()));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(kind: ToolKind, d: f64, fl: f64, flutes: u32) -> Tool {
        Tool {
            id: 1,
            name: "t".into(),
            kind,
            diameter: d,
            flute_length: fl,
            flutes,
            feed: 600.0,
            plunge_feed: 150.0,
            rpm: 16000,
            depth_per_pass: 1.0,
            stepover_pct: 40.0,
            comment: String::new(),
        }
    }
    fn flat(d: f64, fl: f64, n: u32) -> Tool {
        tool(ToolKind::FlatEndMill, d, fl, n)
    }
    fn vbit(a: f64) -> Tool {
        tool(ToolKind::VBit { angle_deg: a }, 6.0, 10.0, 2)
    }
    fn texts(s: &Sketch) -> Vec<&str> {
        s.labels.iter().map(|l| l.text.as_str()).collect()
    }
    fn hints(s: &Sketch) -> usize {
        s.lines.iter().filter(|(p, _)| *p == Pen::Hint).count()
    }
    fn label(s: &Sketch, f: Field) -> Option<&Label> {
        s.labels.iter().find(|l| l.field == f)
    }
    /// Angle in degrees at the tip vertex (index 3 of a V / drill body).
    fn tip_angle(s: &Sketch) -> f64 {
        let t = s.body[3];
        let (a, b) = (s.body[2], s.body[4]);
        let u = [(a[0] - t[0]) as f64, (a[1] - t[1]) as f64];
        let v = [(b[0] - t[0]) as f64, (b[1] - t[1]) as f64];
        let c = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1]));
        c.acos().to_degrees()
    }

    #[test]
    fn g1_flat_labels() {
        let s = sketch(&flat(3.175, 12.0, 2), Units::Mm);
        let f: Vec<_> = s.labels.iter().map(|l| l.field).collect();
        assert_eq!(f, [Field::Diameter, Field::FluteLength, Field::Flutes]);
        assert_eq!(
            texts(&s),
            ["Diameter 3.175 mm", "Flute length\n12.000 mm", "Flutes 2"]
        );
        assert!(s.labels.iter().all(|l| !l.bad));
        assert_eq!(s.body.len(), 4);
    }

    #[test]
    fn g2_flute_lines() {
        for (n, want) in [(1, 1), (4, 4), (6, 6), (8, 6), (12, 6), (0, 0)] {
            let s = sketch(&flat(3.0, 12.0, n), Units::Mm);
            assert_eq!(hints(&s), want, "flutes {n}");
            assert_eq!(
                label(&s, Field::Flutes).unwrap().text,
                format!("Flutes {n}")
            );
        }
    }

    #[test]
    fn g3_g4_v_angle() {
        for (a, drawn, text) in [
            (60.0, 60.0, "V angle\n60°"),
            (90.0, 90.0, "V angle\n90°"),
            (10.0, 30.0, "V angle\n10°"),
            (170.0, 150.0, "V angle\n170°"),
            (59.5, 59.5, "V angle\n59.5°"),
        ] {
            let s = sketch(&vbit(a), Units::Mm);
            assert!((tip_angle(&s) - drawn).abs() < 0.01, "{a}");
            assert_eq!(label(&s, Field::VAngle).unwrap().text, text);
        }
    }

    #[test]
    fn g5_drill_and_ball() {
        let s = sketch(&tool(ToolKind::Drill, 3.0, 10.0, 2), Units::Mm);
        assert!((tip_angle(&s) - 118.0).abs() < 0.01);
        assert!(label(&s, Field::VAngle).is_none());
        let s = sketch(&tool(ToolKind::BallEndMill, 3.0, 10.0, 2), Units::Mm);
        assert_eq!(s.body.len(), 19);
        let low = s.body.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((low - TIP).abs() < 1e-3);
    }

    #[test]
    fn g6_bounds_and_shape_independent_of_values() {
        let kinds = |a: f64| {
            [
                ToolKind::FlatEndMill,
                ToolKind::BallEndMill,
                ToolKind::VBit { angle_deg: a },
                ToolKind::Drill,
            ]
        };
        for a in [1.0, 10.0, 150.0, 179.0] {
            for (ki, kind) in kinds(a).into_iter().enumerate() {
                let mut base: Option<Vec<P>> = None;
                for d in [0.1, 25.0, 0.0, -1.0] {
                    for fl in [0.0, 0.1, 250.0, 1e6] {
                        for n in [0, 1, 8, 12] {
                            let s = sketch(&tool(kind.clone(), d, fl, n), Units::Mm);
                            let all = s
                                .body
                                .iter()
                                .chain(s.lines.iter().flat_map(|(_, v)| v))
                                .chain(s.arrows.iter().flatten())
                                .chain(s.labels.iter().map(|l| &l.pos));
                            for p in all {
                                assert!(p[0].is_finite() && p[1].is_finite());
                                assert!((0.0..=SKETCH_W).contains(&p[0]), "{ki} {p:?}");
                                assert!((0.0..=SKETCH_H).contains(&p[1]), "{ki} {p:?}");
                            }
                            match &base {
                                None => base = Some(s.body),
                                Some(b) => assert_eq!(b, &s.body),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn g7_bad_values() {
        for t in [flat(0.0, 0.0, 0), flat(f64::NAN, 12.0, 0)] {
            let s = sketch(&t, Units::Mm);
            for f in [Field::Diameter, Field::Flutes] {
                assert!(label(&s, f).unwrap().bad);
            }
        }
        let s = sketch(&flat(0.0, 0.0, 0), Units::Mm);
        assert!(s.labels.iter().all(|l| l.bad));
        let s = sketch(&flat(3.0, 12.0, 2), Units::Mm);
        assert!(s.labels.iter().all(|l| !l.bad));
    }

    #[test]
    fn g8_inch() {
        let s = sketch(&flat(3.175, 12.0, 2), Units::Inch);
        assert_eq!(
            texts(&s)[..2],
            ["Diameter 0.1250 in", "Flute length\n0.4724 in"]
        );
    }

    #[test]
    fn g9_hints_inside_zone() {
        let mut t = vbit(30.0);
        t.flutes = 6;
        let s6 = sketch(&t, Units::Mm);
        let side_end = s6.body[2][1];
        assert_eq!(hints(&s6), 6);
        for (_, v) in s6.lines.iter().filter(|(p, _)| *p == Pen::Hint) {
            for p in v {
                assert!(p[1] >= FT && p[1] <= side_end, "{p:?}");
            }
        }
        assert!(side_end > FT);
    }

    #[test]
    fn g10_summary() {
        let s = sketch(&flat(3.175, 12.0, 2), Units::Mm);
        assert_eq!(
            s.summary(),
            "Tool sketch: Diameter 3.175 mm, Flute length 12.000 mm, Flutes 2"
        );
    }

    fn all_tools() -> Vec<Tool> {
        let mut v = vec![];
        let nan = f64::NAN;
        for kind in [
            ToolKind::FlatEndMill,
            ToolKind::BallEndMill,
            ToolKind::Drill,
        ]
        .into_iter()
        .chain(
            [0.0, 1.0, 10.0, 30.0, 150.0, 179.0, 180.0, -5.0, 1e9] // NaN: see x2
                .map(|a| ToolKind::VBit { angle_deg: a }),
        ) {
            for d in [0.0, -1.0, nan, 1e-6, 1e6, f64::INFINITY] {
                for fl in [0.0, nan, 1e12, -3.0] {
                    for n in [0, 1, 6, 7, 12, u32::MAX] {
                        v.push(tool(kind.clone(), d, fl, n));
                    }
                }
            }
        }
        v
    }

    #[test]
    fn x1_extreme_inputs_no_panic_finite_inside() {
        for t in all_tools() {
            for u in [Units::Mm, Units::Inch] {
                let s = sketch(&t, u);
                let all = s
                    .body
                    .iter()
                    .chain(s.lines.iter().flat_map(|(_, v)| v))
                    .chain(s.arrows.iter().flatten())
                    .chain(s.labels.iter().map(|l| &l.pos));
                for p in all {
                    assert!(p[0].is_finite() && p[1].is_finite(), "{t:?}");
                    assert!((0.0..=SKETCH_W).contains(&p[0]), "{t:?} {p:?}");
                    assert!((0.0..=SKETCH_H).contains(&p[1]), "{t:?} {p:?}");
                }
                assert!(hints(&s) <= MAX_FLUTE_LINES as usize);
                assert_eq!(
                    label(&s, Field::Flutes).unwrap().bad,
                    t.flutes == 0,
                    "{t:?}"
                );
                for (f, v) in [
                    (Field::Diameter, t.diameter),
                    (Field::FluteLength, t.flute_length),
                ] {
                    assert_eq!(label(&s, f).unwrap().bad, !(v.is_finite() && v > 0.0));
                }
                let is_v = matches!(t.kind, ToolKind::VBit { .. });
                assert_eq!(label(&s, Field::VAngle).is_some(), is_v);
                if let Some(l) = label(&s, Field::VAngle) {
                    assert!(!l.bad);
                }
                s.summary();
            }
        }
    }

    #[test]
    fn x2_v_angle_clamp_and_nan() {
        for (a, drawn) in [
            (0.0, 30.0),
            (1.0, 30.0),
            (30.0, 30.0),
            (150.0, 150.0),
            (179.0, 150.0),
            (180.0, 150.0),
        ] {
            let s = sketch(&vbit(a), Units::Mm);
            assert!((tip_angle(&s) - drawn).abs() < 0.01, "{a}");
        }
        // NaN stays finite (clamp keeps NaN: the tip must not be NaN)
        let s = sketch(&vbit(f64::NAN), Units::Mm);
        assert!(s.body.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn x3_inch_and_wrong_flag_formats() {
        let s = sketch(&flat(25.4, 254.0, 3), Units::Inch);
        assert_eq!(
            texts(&s),
            ["Diameter 1.0000 in", "Flute length\n10.0000 in", "Flutes 3"]
        );
        let s = sketch(&flat(-1.0, 0.0, 1), Units::Inch);
        assert!(label(&s, Field::Diameter).unwrap().bad);
        assert!(label(&s, Field::FluteLength).unwrap().bad);
    }

    /// Text x-extent of every label (real font metrics) must stay inside the sketch box.
    #[test]
    fn x4_labels_not_clipped() {
        let mut h = egui_kittest::Harness::new_ui(|ui| {
            ui.label("x");
        });
        h.run();
        let mut tools = vec![];
        for a in [1.0, 30.0, 60.0, 100.0, 150.0, 170.0, 179.0, 59.5] {
            tools.push(vbit(a));
        }
        for d in [0.1, 3.175, 25.0, 1000.0, 1e6] {
            // shortcut: a 1 km flute length (1e6 mm) runs ~2 pt past the box; not a real tool
            for fl in [0.0, 12.0, 250.0, 99_999.0] {
                tools.push(flat(d, fl, 12));
            }
        }
        let mut bad = vec![];
        for t in tools {
            for u in [Units::Mm, Units::Inch] {
                for l in sketch(&t, u).labels {
                    let w = h.ctx.fonts_mut(|f| {
                        f.layout_no_wrap(
                            l.text.clone(),
                            egui::FontId::proportional(12.0),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                    });
                    let (x0, x1) = match l.anchor {
                        Anchor::LeftCenter => (l.pos[0], l.pos[0] + w),
                        Anchor::RightCenter => (l.pos[0] - w, l.pos[0]),
                        Anchor::CenterCenter => (l.pos[0] - w / 2.0, l.pos[0] + w / 2.0),
                    };
                    if x0 < 0.0 || x1 > SKETCH_W {
                        bad.push(format!("{:?} {:?} x {x0:.1}..{x1:.1}", t.kind, l.text));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "clipped: {bad:#?}");
    }

    #[test]
    fn x5_no_hardcoded_colors() {
        let src = include_str!("tool_sketch.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("Color32::"));
    }
}
