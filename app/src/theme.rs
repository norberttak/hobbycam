//! M18: colour palette, sidebar tile colours, egui visuals and canvas colours (one source for light and dark).

use eframe::egui::{Color32, Stroke, Visuals};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Sidebar tile colour group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Draw,
    View,
    Edit,
    File,
    Ops,
    Set,
}

/// B palette, one per theme.
pub struct Palette {
    pub win: Color32,
    pub panel: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub head: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub btn: Color32,
    pub input: Color32,
    pub rowsel: Color32,
    pub canvas: Color32,
}

pub const LIGHT: Palette = Palette {
    win: hex(0xEEF1F5),
    panel: hex(0xFAFBFC),
    accent: hex(0x0EA5E9),
    text: hex(0x1E293B),
    head: hex(0x0F172A),
    muted: hex(0x64748B),
    border: hex(0xD8DEE7),
    btn: hex(0xE2E8F0),
    input: hex(0xFFFFFF),
    rowsel: hex(0xE0F2FE),
    canvas: hex(0xFFFFFF),
};

pub const DARK: Palette = Palette {
    win: hex(0x181B21),
    panel: hex(0x1F232B),
    accent: hex(0x38BDF8),
    text: hex(0xE2E8F0),
    head: hex(0xF8FAFC),
    muted: hex(0x94A3B8),
    border: hex(0x2E343E),
    btn: hex(0x2A303A),
    input: hex(0x171A20),
    rowsel: hex(0x0C4A6E),
    canvas: hex(0x111418),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

pub fn group_color(t: Tint, dark: bool) -> Color32 {
    let (l, d) = match t {
        Tint::Draw => (0x3B82F6, 0x2563EB),
        Tint::View => (0x22A06B, 0x15803D),
        Tint::Edit => (0x8B5CF6, 0x7C3AED),
        Tint::File => (0x64748B, 0x475569),
        Tint::Ops => (0xEA580C, 0xEA580C),
        Tint::Set => (0x71717A, 0x52525B),
    };
    hex(if dark { d } else { l })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileStyle {
    pub fill: Color32,
    pub icon: Color32,
    /// Selected: (gap colour = panel, ring colour = head).
    pub ring: Option<(Color32, Color32)>,
}

pub fn tile_style(
    t: Tint,
    dark: bool,
    enabled: bool,
    selected: bool,
    hovered: bool,
    pressed: bool,
) -> TileStyle {
    let p = palette(dark);
    if !enabled {
        return TileStyle {
            fill: p.btn,
            icon: p.muted,
            ring: None,
        };
    }
    let base = group_color(t, dark);
    let fill = if pressed {
        brightness(base, 0.88)
    } else if hovered {
        brightness(base, 1.12)
    } else {
        base
    };
    TileStyle {
        fill,
        icon: Color32::WHITE,
        ring: selected.then_some((p.panel, p.head)),
    }
}

/// Each RGB channel x f, saturating at 255 (CSS `brightness()`); alpha 255.
pub fn brightness(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).round().min(255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

pub fn visuals(dark: bool) -> Visuals {
    let p = palette(dark);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.panel;
    v.window_fill = p.win;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.faint_bg_color = p.win;
    v.extreme_bg_color = p.canvas;
    v.text_edit_bg_color = Some(p.input);
    v.weak_text_color = Some(p.muted);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.rowsel;
    v.selection.stroke = Stroke::new(1.0, p.head);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = p.btn;
    w.inactive.weak_bg_fill = p.btn;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_fill = p.btn;
    w.hovered.weak_bg_fill = p.btn;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5, p.head);
    w.active.bg_fill = p.rowsel;
    w.active.weak_bg_fill = p.rowsel;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(2.0, p.head);
    w.open.bg_fill = p.rowsel;
    w.open.weak_bg_fill = p.rowsel;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    v
}

/// Canvas colours not covered by `Visuals`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasPalette {
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub selected: Color32,
    pub snap: Color32,
    pub feed: Color32,
    /// Rapids and the stock outline.
    pub muted: Color32,
    /// Measure ruler and its label.
    pub measure: Color32,
}

pub fn canvas_palette(dark: bool) -> CanvasPalette {
    if dark {
        CanvasPalette {
            grid_minor: hex(0x20252D),
            grid_major: hex(0x30363F),
            selected: hex(0xFF9F43),
            snap: hex(0x22D3EE),
            feed: hex(0x5B9BFF),
            muted: hex(0x94A3B8),
            measure: hex(0xF472B6),
        }
    } else {
        CanvasPalette {
            grid_minor: hex(0xE6EAF0),
            grid_major: hex(0xC9D1DC),
            selected: hex(0xFF7A00),
            snap: hex(0x00B8D9),
            measure: hex(0xDB2777),
            feed: hex(0x1D6FE0),
            muted: hex(0x64748B),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINTS: [Tint; 6] = [
        Tint::Draw,
        Tint::View,
        Tint::Edit,
        Tint::File,
        Tint::Ops,
        Tint::Set,
    ];

    #[test]
    fn palette_values() {
        assert_eq!(LIGHT.panel, hex(0xFAFBFC));
        assert_eq!(DARK.panel, hex(0x1F232B));
        assert_eq!(LIGHT.head, hex(0x0F172A));
        assert_eq!(DARK.canvas, hex(0x111418));
        assert_eq!(group_color(Tint::Ops, false), hex(0xEA580C));
        assert_eq!(group_color(Tint::View, true), hex(0x15803D));
        for dark in [false, true] {
            for (i, a) in TINTS.iter().enumerate() {
                for b in &TINTS[i + 1..] {
                    assert_ne!(group_color(*a, dark), group_color(*b, dark));
                }
            }
        }
    }

    #[test]
    fn tile_normal() {
        let s = tile_style(Tint::Draw, false, true, false, false, false);
        assert_eq!(s.fill, hex(0x3B82F6));
        assert_eq!(s.icon, Color32::WHITE);
        assert_eq!(s.ring, None);
    }

    #[test]
    fn tile_selected() {
        let s = tile_style(Tint::Edit, true, true, true, false, false);
        assert_eq!(s.fill, hex(0x7C3AED));
        assert_eq!(s.ring, Some((hex(0x1F232B), hex(0xF8FAFC))));
    }

    #[test]
    fn tile_disabled() {
        let s = tile_style(Tint::Edit, false, false, false, true, true);
        assert_eq!(s.fill, hex(0xE2E8F0));
        assert_eq!(s.icon, hex(0x64748B));
        assert_eq!(s.ring, None);
        let s = tile_style(Tint::Ops, false, false, true, false, false);
        assert_eq!(s.ring, None);
    }

    #[test]
    fn tile_hover_pressed() {
        let h = tile_style(Tint::Draw, false, true, false, true, false);
        assert_eq!(h.fill, Color32::from_rgb(66, 146, 255));
        let p = tile_style(Tint::Draw, false, true, false, true, true);
        assert_eq!(p.fill, Color32::from_rgb(52, 114, 216));
        assert_eq!(
            brightness(Color32::from_rgb(200, 100, 0), 1.5),
            Color32::from_rgb(255, 150, 0)
        );
    }

    #[test]
    fn visuals_light_dark() {
        for dark in [false, true] {
            let p = palette(dark);
            let v = visuals(dark);
            assert_eq!(v.dark_mode, dark);
            assert_eq!(v.panel_fill, p.panel);
            assert_eq!(v.window_fill, p.win);
            assert_eq!(v.extreme_bg_color, p.canvas);
            assert_eq!(v.text_edit_bg_color, Some(p.input));
            assert_eq!(v.selection.bg_fill, p.rowsel);
            assert_eq!(v.hyperlink_color, p.accent);
            assert_eq!(v.widgets.inactive.weak_bg_fill, p.btn);
            assert_eq!(v.widgets.noninteractive.bg_stroke.color, p.border);
        }
    }

    #[test]
    fn canvas_palette_values() {
        let l = canvas_palette(false);
        let d = canvas_palette(true);
        assert_eq!(l.grid_minor, hex(0xE6EAF0));
        assert_eq!(l.grid_major, hex(0xC9D1DC));
        assert_eq!(l.selected, hex(0xFF7A00));
        assert_eq!(l.snap, hex(0x00B8D9));
        assert_eq!(l.feed, hex(0x1D6FE0));
        assert_eq!(l.muted, hex(0x64748B));
        assert_eq!(d.grid_minor, hex(0x20252D));
        assert_eq!(d.grid_major, hex(0x30363F));
        assert_eq!(d.selected, hex(0xFF9F43));
        assert_eq!(d.snap, hex(0x22D3EE));
        assert_eq!(d.feed, hex(0x5B9BFF));
        assert_eq!(d.muted, hex(0x94A3B8));
        for c in [l, d] {
            assert_ne!(c.grid_minor, c.grid_major);
            assert_ne!(c.selected, c.feed);
        }
    }
}
