//! Phosphor icon font (regular, MIT): https://unpkg.com/@phosphor-icons/web@2.1.2/src/regular/Phosphor.ttf
//! Codepoints from the `style.css` of the same release; see `assets/README.md`.

use eframe::egui::{self, FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};

pub const NEW: &str = "\u{e236}"; // file-plus
pub const OPEN: &str = "\u{e256}"; // folder-open
pub const SAVE: &str = "\u{e248}"; // floppy-disk
pub const IMPORT: &str = "\u{e232}"; // file-arrow-down
pub const EXPORT: &str = "\u{eaf0}"; // export
pub const UNDO: &str = "\u{e08a}"; // arrow-u-up-left
pub const REDO: &str = "\u{e08c}"; // arrow-u-up-right
pub const DELETE: &str = "\u{e4a6}"; // trash
pub const ROTATE: &str = "\u{e038}"; // arrow-counter-clockwise
pub const FLIP_H: &str = "\u{ed6a}"; // flip-horizontal
pub const FLIP_V: &str = "\u{ed6c}"; // flip-vertical
pub const SELECT: &str = "\u{e1dc}"; // cursor
pub const LINE: &str = "\u{e6d2}"; // line-segment
pub const RECT: &str = "\u{e3f0}"; // rectangle
pub const CIRCLE: &str = "\u{e18a}"; // circle
pub const ORIGIN: &str = "\u{e1d6}"; // crosshair
pub const FIT: &str = "\u{e1d0}"; // corners-out
pub const TOOLPATHS: &str = "\u{e39c}"; // path
pub const SIM: &str = "\u{e3d0}"; // play
pub const UNITS: &str = "\u{e6b8}"; // ruler
pub const MACHINE: &str = "\u{e270}"; // gear
pub const STOCK: &str = "\u{e1da}"; // cube
pub const TOOLS: &str = "\u{e5d4}"; // wrench

#[cfg(test)]
pub const ALL: [&str; 23] = [
    NEW, OPEN, SAVE, IMPORT, EXPORT, UNDO, REDO, DELETE, ROTATE, FLIP_H, FLIP_V, SELECT, LINE,
    RECT, CIRCLE, ORIGIN, FIT, TOOLPATHS, SIM, UNITS, MACHINE, STOCK, TOOLS,
];

/// Adds Phosphor as the lowest-priority fallback of the proportional family (Private Use Area, no clash with text).
pub fn install(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        "phosphor",
        FontData::from_static(include_bytes!("../assets/Phosphor.ttf")),
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Lowest,
        }],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M10a.2 test 13.
    #[test]
    fn every_icon_has_a_glyph() {
        let mut h = egui_kittest::Harness::new_ui(|ui| {
            ui.label("icons");
        });
        install(&h.ctx);
        h.run(); // fonts are rebuilt at the start of a frame
        let id = egui::FontId::proportional(24.0);
        h.ctx.fonts_mut(|f| {
            for icon in ALL {
                let c = icon.chars().next().unwrap();
                assert!(f.has_glyph(&id, c), "U+{:04X}", c as u32);
            }
        });
    }
}
