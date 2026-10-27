//! Drawing canvas: input (pan/zoom/select) and painting.

use crate::hit::{segment_points, tessellate_arc};
use crate::snap::SNAP_PX;
use crate::state::{AppState, CanvasTool};
use crate::view::grid_step;
use crate::{aci, App};
use eframe::egui::{
    self, Color32, MouseWheelUnit, Painter, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui,
    Vec2,
};
use hc_geom::{BBox, Point};
use hc_project::Geom;
use hc_toolpath::Move;

const PICK_PX: f64 = 6.0;
const ARC_TOL_PX: f64 = 0.25;
const BOX_MIN_PX: f32 = 4.0;
const WHEEL_ZOOM: f32 = 1.2;
const SELECTED: Color32 = Color32::from_rgb(255, 140, 0);
const SNAP: Color32 = Color32::from_rgb(0, 190, 255);

pub fn show(app: &mut App, ui: &mut Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    if app.state.fit_pending && rect.width() > 1.0 && rect.height() > 1.0 {
        let (b, s) = (app.state.items_bbox(), app.state.stock_bbox());
        app.state.view.fit(rect, &b, &s);
        app.state.fit_pending = false;
    }
    input(app, ui, &resp);
    paint(app, ui, &painter.with_clip_rect(rect), rect);
}

fn input(app: &mut App, ui: &Ui, resp: &egui::Response) {
    let rect = resp.rect;
    let s = &mut app.state;
    let hover = resp.hover_pos();
    s.cursor = hover.map(|p| s.view.screen_to_world(rect, p));
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    let shift = ui.input(|i| i.modifiers.shift);

    // pan
    if resp.dragged_by(PointerButton::Middle) || (space && resp.dragged_by(PointerButton::Primary))
    {
        s.view.offset += resp.drag_delta();
    }

    if let Some(p) = hover {
        let events = ui.input(|i| i.events.clone());
        for e in events {
            if let egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } = e
            {
                if modifiers.command || modifiers.ctrl {
                    continue; // handled by zoom_delta below
                }
                match unit {
                    MouseWheelUnit::Point => s.view.offset += delta,
                    _ => s.view.zoom_at(rect, p, WHEEL_ZOOM.powf(delta.y)),
                }
            }
        }
        let zd = ui.input(|i| i.zoom_delta());
        if (zd - 1.0).abs() > f32::EPSILON {
            s.view.zoom_at(rect, p, zd);
        }
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui.input(|i| i.key_pressed(egui::Key::F)) {
                s.fit_pending = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                s.escape();
            }
            // plain letters only: Cmd+O etc. are menu shortcuts
            let plain = ui.input(|i| !i.modifiers.any());
            for (key, tool) in [
                (egui::Key::L, CanvasTool::Line),
                (egui::Key::R, CanvasTool::Rect),
                (egui::Key::C, CanvasTool::Circle),
                (egui::Key::O, CanvasTool::SetOrigin),
            ] {
                if plain && ui.input(|i| i.key_pressed(key)) {
                    s.set_tool(tool);
                }
            }
            let del = ui
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
            if del {
                match s.selected_tab {
                    Some((item, idx)) => s.delete_tab(item, idx),
                    None => s.delete_selection(),
                }
            }
        }
    }

    let zoom = s.view.zoom as f64;
    let pick = PICK_PX / zoom;
    if s.tool != CanvasTool::Select {
        // tool clicks only: no selection, box or tab editing
        if resp.clicked_by(PointerButton::Primary) && !space {
            if let Some(p) = resp.interact_pointer_pos() {
                let w = s.view.screen_to_world(rect, p);
                let snap = s.snap_at(w, SNAP_PX / zoom);
                s.canvas_click(snap.map_or(w, |sn| sn.p), snap.is_some());
            }
        }
        app.box_start = None;
        app.tab_drag = false;
        return;
    }
    if resp.double_clicked_by(PointerButton::Primary) && !space {
        if let Some(p) = resp.interact_pointer_pos() {
            let w = s.view.screen_to_world(rect, p);
            let near = s
                .tab_markers()
                .iter()
                .map(|(id, c, _)| (*id, c.distance_to(w)))
                .filter(|(_, d)| *d <= pick)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            if let Some(id) = near {
                s.add_tab(id, w);
            }
        }
    } else if resp.clicked_by(PointerButton::Primary) && !space {
        if let Some(p) = resp.interact_pointer_pos() {
            let w = s.view.screen_to_world(rect, p);
            s.selected_tab = s.tab_at(w, pick);
            if s.selected_tab.is_none() {
                s.click_select(w, pick, shift);
            }
        }
    }
    if resp.drag_started_by(PointerButton::Primary) && !space {
        let origin = ui.input(|i| i.pointer.press_origin());
        let tab = origin.and_then(|o| s.tab_at(s.view.screen_to_world(rect, o), pick));
        if tab.is_some() {
            s.selected_tab = tab;
            app.tab_drag = true;
        } else {
            app.box_start = origin;
        }
    }
    if app.tab_drag {
        match (s.selected_tab, resp.interact_pointer_pos()) {
            (Some((item, idx)), Some(p)) if resp.dragged_by(PointerButton::Primary) => {
                s.move_tab(item, idx, s.view.screen_to_world(rect, p));
            }
            _ => app.tab_drag = false,
        }
    }
    if resp.drag_stopped() {
        if let (Some(a), Some(b)) = (app.box_start.take(), resp.interact_pointer_pos()) {
            if (b - a).length() > BOX_MIN_PX {
                let (wa, wb) = (
                    s.view.screen_to_world(rect, a),
                    s.view.screen_to_world(rect, b),
                );
                s.box_select(wa, wb, shift, ARC_TOL_PX / zoom);
            }
        }
    }
    if !resp.dragged() && !resp.drag_stopped() {
        app.box_start = None;
    }
}

fn paint(app: &App, ui: &Ui, painter: &Painter, rect: Rect) {
    let s = &app.state;
    let v = &s.view;
    let dark = ui.visuals().dark_mode;
    let vis = v.visible_world(rect);

    // grid
    let step = grid_step(v.zoom, s.project.units, 15.0);
    let weak = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let light = Stroke::new(1.0, weak.gamma_multiply(0.4));
    let strong = Stroke::new(1.0, weak);
    let i0 = (vis.min.x / step).floor() as i64;
    let i1 = (vis.max.x / step).ceil() as i64;
    for i in i0..=i1 {
        let x = v.world_to_screen(rect, Point::new(i as f64 * step, 0.0)).x;
        let st = if i % 10 == 0 { strong } else { light };
        painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], st);
    }
    let j0 = (vis.min.y / step).floor() as i64;
    let j1 = (vis.max.y / step).ceil() as i64;
    for j in j0..=j1 {
        let y = v.world_to_screen(rect, Point::new(0.0, j as f64 * step)).y;
        let st = if j % 10 == 0 { strong } else { light };
        painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], st);
    }
    let o = v.world_to_screen(rect, Point::new(0.0, 0.0));
    let red = Color32::from_rgb(220, 50, 50);
    let green = Color32::from_rgb(40, 170, 40);
    painter.line_segment(
        [Pos2::new(rect.min.x, o.y), Pos2::new(rect.max.x, o.y)],
        Stroke::new(1.0, red.gamma_multiply(0.6)),
    );
    painter.line_segment(
        [Pos2::new(o.x, rect.min.y), Pos2::new(o.x, rect.max.y)],
        Stroke::new(1.0, green.gamma_multiply(0.6)),
    );

    // stock
    let sb = s.stock_bbox();
    let corners = [
        sb.min,
        Point::new(sb.max.x, sb.min.y),
        sb.max,
        Point::new(sb.min.x, sb.max.y),
        sb.min,
    ]
    .map(|p| v.world_to_screen(rect, p));
    painter.extend(Shape::dashed_line(
        &corners,
        Stroke::new(1.0, Color32::GRAY),
        6.0,
        4.0,
    ));

    let sim = s
        .sim
        .as_ref()
        .ok()
        .filter(|sim| s.sim_view.open && !sim.steps.is_empty());
    if let Some(sim) = sim {
        sim_band(s, sim, painter, rect);
    }

    // origin marker at the G-code work zero
    let o = v.world_to_screen(rect, hc_post::work_origin(&s.project));
    painter.arrow(o, Vec2::new(20.0, 0.0), Stroke::new(2.0, red));
    painter.arrow(o, Vec2::new(0.0, -20.0), Stroke::new(2.0, green));

    // items: normal, then selected on top
    let tol = ARC_TOL_PX / v.zoom as f64;
    let margin = 2.0 / v.zoom as f64;
    let in_view = |b: &BBox| {
        !b.is_empty()
            && b.max.x >= vis.min.x - margin
            && b.min.x <= vis.max.x + margin
            && b.max.y >= vis.min.y - margin
            && b.min.y <= vis.max.y + margin
    };
    for selected in [false, true] {
        if selected && s.show_toolpaths {
            toolpaths(s, painter, rect, tol, dark);
        }
        for (it, b) in s.project.items.iter().zip(&s.item_bbox) {
            if selected != s.selection.contains(&it.id) || !s.is_visible(it) || !in_view(b) {
                continue;
            }
            let stroke = if selected {
                Stroke::new(2.5, SELECTED)
            } else {
                let c = s
                    .project
                    .layers
                    .iter()
                    .find(|l| l.name == it.layer)
                    .map_or(7, |l| l.color);
                Stroke::new(1.0, aci::aci_rgb(c, dark))
            };
            let px = (b.width().max(b.height()) * v.zoom as f64) as f32;
            match &it.geom {
                Geom::Point(p) => {
                    let c = v.world_to_screen(rect, *p);
                    painter
                        .line_segment([c - Vec2::new(4.0, 0.0), c + Vec2::new(4.0, 0.0)], stroke);
                    painter
                        .line_segment([c - Vec2::new(0.0, 4.0), c + Vec2::new(0.0, 4.0)], stroke);
                }
                Geom::Contour(_) if px < 2.0 => {
                    let c = v.world_to_screen(
                        rect,
                        Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0),
                    );
                    painter.circle_filled(c, stroke.width.max(1.0), stroke.color);
                }
                Geom::Contour(c) => {
                    let mut pts: Vec<Pos2> = Vec::new();
                    for seg in &c.segments {
                        let sp = segment_points(seg, tol);
                        // skip the joint point shared with the previous segment
                        let skip = usize::from(!pts.is_empty());
                        pts.extend(
                            sp.into_iter()
                                .skip(skip)
                                .map(|p| v.world_to_screen(rect, p)),
                        );
                    }
                    if c.closed {
                        pts.pop(); // closed_line adds the closing edge
                        painter.add(Shape::closed_line(pts, stroke));
                    } else {
                        painter.add(Shape::line(pts, stroke));
                    }
                }
            }
        }
    }

    tab_markers(s, painter, rect, dark, ui.visuals().text_color());
    // tool marker on top
    if let Some((sim, i)) = sim.and_then(|sim| Some((sim, sim.step_at(s.sim_view.t)?))) {
        let st = &sim.steps[i];
        let c = v.world_to_screen(rect, st.point_at(s.sim_view.t).xy());
        let r = (tool_d(s, st.tool) / 2.0 * v.zoom as f64) as f32;
        let stroke = Stroke::new(2.0, ui.visuals().text_color());
        painter.circle_stroke(c, r, stroke);
        painter.line_segment([c - Vec2::new(6.0, 0.0), c + Vec2::new(6.0, 0.0)], stroke);
        painter.line_segment([c - Vec2::new(0.0, 6.0), c + Vec2::new(0.0, 6.0)], stroke);
    }

    if s.tool != CanvasTool::Select {
        tool_overlay(s, ui, painter, rect);
    }

    // selection box
    if let (Some(a), Some(b)) = (app.box_start, ui.input(|i| i.pointer.latest_pos())) {
        if (b - a).length() > BOX_MIN_PX {
            let r = Rect::from_two_pos(a, b);
            if b.x >= a.x {
                let blue = Color32::from_rgb(60, 120, 255);
                painter.rect_filled(r, 0.0, blue.gamma_multiply(0.1));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, blue), egui::StrokeKind::Inside);
            } else {
                let green = Color32::from_rgb(40, 180, 80);
                painter.rect_filled(r, 0.0, green.gamma_multiply(0.1));
                let pts = [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ];
                painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, green), 5.0, 4.0));
            }
        }
    }
}

/// Snap marker with its kind label and the rubber band from the pending first point.
fn tool_overlay(s: &AppState, ui: &Ui, painter: &Painter, rect: Rect) {
    let v = &s.view;
    let Some(cursor) = s.cursor else { return };
    let snap = s.snap_at(cursor, SNAP_PX / v.zoom as f64);
    let at = snap.map_or(cursor, |sn| sn.p);
    let stroke = Stroke::new(1.0, SNAP);
    if let Some(sn) = snap {
        let c = v.world_to_screen(rect, sn.p);
        painter.rect_stroke(
            Rect::from_center_size(c, Vec2::splat(8.0)),
            0.0,
            stroke,
            egui::StrokeKind::Middle,
        );
        painter.text(
            c + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            sn.kind.label(),
            egui::TextStyle::Body.resolve(ui.style()),
            SNAP,
        );
    }
    let Some(first) = s.pending else { return };
    let world: Vec<Point> = match s.tool {
        CanvasTool::Line => vec![first, at],
        CanvasTool::Rect => vec![
            first,
            Point::new(at.x, first.y),
            at,
            Point::new(first.x, at.y),
            first,
        ],
        CanvasTool::Circle => {
            let r = first.distance(at);
            if r * (v.zoom as f64) < 1.0 {
                return;
            }
            tessellate_arc(
                first,
                r,
                0.0,
                std::f64::consts::TAU,
                ARC_TOL_PX / v.zoom as f64,
            )
        }
        CanvasTool::Select | CanvasTool::SetOrigin => return,
    };
    let pts: Vec<Pos2> = world
        .into_iter()
        .map(|p| v.world_to_screen(rect, p))
        .collect();
    painter.extend(Shape::dashed_line(&pts, stroke, 5.0, 4.0));
}

/// Top view of the toolpaths: feed moves solid, XY rapids dashed, drills as tool circle + cross. Z is not shown.
fn toolpaths(s: &AppState, painter: &Painter, rect: Rect, tol: f64, dark: bool) {
    let v = &s.view;
    let feed = if dark {
        Color32::from_rgb(80, 170, 255)
    } else {
        Color32::from_rgb(0, 90, 210)
    };
    let feed_stroke = Stroke::new(1.5, feed);
    let rapid_stroke = Stroke::new(1.0, Color32::GRAY);
    let to_screen = |p: Point| v.world_to_screen(rect, p);
    for tp in s.toolpaths.iter().filter_map(|(_, r)| r.as_ref().ok()) {
        let tool_r = s
            .project
            .tools
            .iter()
            .find(|t| t.id == tp.tool_id)
            .map_or(0.0, |t| t.diameter / 2.0);
        let mut pos: Option<Point> = None;
        let mut run: Vec<Pos2> = Vec::new();
        let flush = |run: &mut Vec<Pos2>| {
            if run.len() > 1 {
                painter.add(Shape::line(std::mem::take(run), feed_stroke));
            }
            run.clear();
        };
        for m in &tp.moves {
            match m {
                Move::Rapid(to) => {
                    flush(&mut run);
                    let to = to.xy();
                    if let Some(from) = pos.filter(|f| !f.approx_eq(to, 1e-9)) {
                        painter.extend(Shape::dashed_line(
                            &[to_screen(from), to_screen(to)],
                            rapid_stroke,
                            4.0,
                            4.0,
                        ));
                    }
                    pos = Some(to);
                }
                Move::Linear { to, .. } => {
                    let to = to.xy();
                    if let Some(from) = pos.filter(|f| !f.approx_eq(to, 1e-9)) {
                        if run.is_empty() {
                            run.push(to_screen(from));
                        }
                        run.push(to_screen(to));
                    }
                    pos = Some(to);
                }
                Move::Arc {
                    to, center, ccw, ..
                } => {
                    let to = to.xy();
                    if let Some(from) = pos {
                        let d = from - *center;
                        let a0 = d.y.atan2(d.x);
                        let a1 = (to.y - center.y).atan2(to.x - center.x);
                        let sweep = if *ccw {
                            (a1 - a0).rem_euclid(std::f64::consts::TAU)
                        } else {
                            -(a0 - a1).rem_euclid(std::f64::consts::TAU)
                        };
                        let pts = tessellate_arc(*center, d.length(), a0, sweep, tol);
                        let skip = usize::from(!run.is_empty());
                        run.extend(pts.into_iter().skip(skip).map(to_screen));
                    }
                    pos = Some(to);
                }
                Move::Drill { at, .. } => {
                    flush(&mut run);
                    let c = to_screen(*at);
                    let r = (tool_r * v.zoom as f64) as f32;
                    painter.circle_stroke(c, r, feed_stroke);
                    let k = r.max(4.0);
                    painter
                        .line_segment([c - Vec2::new(k, 0.0), c + Vec2::new(k, 0.0)], feed_stroke);
                    painter
                        .line_segment([c - Vec2::new(0.0, k), c + Vec2::new(0.0, k)], feed_stroke);
                    pos = Some(*at);
                }
            }
        }
        flush(&mut run);
    }
}

fn tool_d(s: &AppState, id: u32) -> f64 {
    s.project
        .tools
        .iter()
        .find(|t| t.id == id)
        .map_or(0.0, |t| t.diameter)
}

/// Opaque depth colour: (255, 210, 80) at Z 0 to (170, 40, 40) at Z = −thickness, clamped.
fn depth_color(z: f64, thickness: f64) -> Color32 {
    let f = if thickness > 0.0 {
        (-z / thickness).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let mix = |a: f64, b: f64| (a + (b - a) * f).round() as u8;
    Color32::from_rgb(mix(255.0, 170.0), mix(210.0, 40.0), mix(80.0, 40.0))
}

/// Swept band of the cutting feed moves up to `sim_view.t`: one wide line per step plus a disc at its end
/// (round joins; egui's miter joins spike at sharp turns).
fn sim_band(s: &AppState, sim: &hc_sim::Sim, painter: &Painter, rect: Rect) {
    let v = &s.view;
    let t = s.sim_view.t;
    let tol = ARC_TOL_PX / v.zoom as f64;
    let thickness = s.project.stock.thickness;
    let Some(last) = sim.step_at(t) else { return };
    // shortcut: repaints every step each frame; cache the completed steps' screen points if big pockets lag
    for st in &sim.steps[..=last] {
        let end = st.point_at(t);
        let min_z = st.from.z.min(end.z);
        if st.kind != hc_sim::StepKind::Feed || min_z >= -1e-6 || t <= st.t0 {
            continue;
        }
        let pts: Vec<Point> = match st.arc {
            None => vec![st.from.xy(), end.xy()],
            Some((c, sweep)) => {
                let d = st.from.xy() - c;
                let f = ((t - st.t0) / (st.t1 - st.t0)).min(1.0);
                tessellate_arc(c, d.length(), d.y.atan2(d.x), sweep * f, tol)
            }
        };
        let d = tool_d(s, st.tool) * v.zoom as f64;
        let col = depth_color(min_z, thickness);
        let pts: Vec<Pos2> = pts
            .into_iter()
            .map(|p| v.world_to_screen(rect, p))
            .collect();
        if let Some(&e) = pts.last() {
            painter.circle_filled(e, (d / 2.0) as f32, col);
        }
        painter.add(Shape::line(pts, Stroke::new(d.max(1.0) as f32, col)));
    }
}

/// Tab markers of the selected op: a bar of the tab width along the edge plus a dot at the centre.
/// The selected tab is a larger dot outlined in `outline` (the text colour), so it stands out on a selected contour.
fn tab_markers(s: &AppState, painter: &Painter, rect: Rect, dark: bool, outline: Color32) {
    let teal = if dark {
        Color32::from_rgb(0, 210, 190)
    } else {
        Color32::from_rgb(0, 128, 128)
    };
    let Some(w) = s
        .selected_op
        .and_then(|id| s.project.operations.iter().find(|o| o.id == id))
        .map(|o| o.tabs.width)
    else {
        return;
    };
    for (id, c, pos) in s.tab_markers() {
        let l = c.length();
        for (i, t) in pos.iter().enumerate() {
            let at = t.rem_euclid(1.0) * l;
            let selected = s.selected_tab == Some((id, i));
            let col = if selected { SELECTED } else { teal };
            let a = s.view.world_to_screen(rect, c.point_at(at - w / 2.0));
            let b = s.view.world_to_screen(rect, c.point_at(at + w / 2.0));
            painter.line_segment([a, b], Stroke::new(4.0, col));
            let centre = s.view.world_to_screen(rect, c.point_at(at));
            if selected {
                painter.circle(centre, 7.0, col, Stroke::new(2.0, outline));
            } else {
                painter.circle_filled(centre, 5.0, col);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_color_lerps_and_clamps() {
        assert_eq!(depth_color(0.0, 10.0), Color32::from_rgb(255, 210, 80));
        assert_eq!(depth_color(-10.0, 10.0), Color32::from_rgb(170, 40, 40));
        assert_eq!(depth_color(-20.0, 10.0), Color32::from_rgb(170, 40, 40));
        assert_eq!(depth_color(-5.0, 10.0), Color32::from_rgb(213, 125, 60));
        assert_eq!(depth_color(-1.0, 0.0), Color32::from_rgb(170, 40, 40));
    }
}
