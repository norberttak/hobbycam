//! Text as geometry (M10b): Hershey Roman Simplex single-line strokes (engraving) and Ubuntu Light outlines (pockets/contours).
//! One line, advance widths only (no kerning), mm. See specs/M10.md.

mod hershey;
mod outline;

use hc_geom::{Contour, Point};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// Roman Simplex, open strokes.
    Hershey,
    /// Ubuntu Light, closed outlines.
    Outline,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextGeom {
    pub contours: Vec<Contour>,
    /// In text order, without duplicates.
    pub missing: Vec<char>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextError {
    BadHeight,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::BadHeight => write!(f, "Text height must be greater than 0"),
        }
    }
}

impl std::error::Error for TextError {}

/// Curve flattening tolerance, mm (PLAN default).
pub const FLATTEN_TOL: f64 = 0.01;

/// One glyph placed with its baseline start at `origin`: contours and advance (mm); None if the font lacks it.
type Placed = Option<(Vec<Contour>, f64)>;

/// One line of text; `height` = cap height (of `H`), `at` = baseline start of the first glyph. A '\n' counts as missing.
/// Missing characters advance by the space width. Deterministic; the font is parsed once.
pub fn text(s: &str, font: Font, height: f64, at: Point) -> Result<TextGeom, TextError> {
    if !(height.is_finite() && height > 0.0) {
        return Err(TextError::BadHeight);
    }
    let (glyph, space): (fn(char, f64, Point) -> Placed, f64) = match font {
        Font::Hershey => (hershey::glyph, hershey::space(height)),
        Font::Outline => (outline::glyph, outline::space(height)),
    };
    let mut g = TextGeom::default();
    let mut pen = at.x;
    for c in s.chars() {
        let placed = if c.is_control() {
            None
        } else {
            glyph(c, height, Point::new(pen, at.y))
        };
        match placed {
            Some((contours, adv)) => {
                g.contours.extend(contours);
                pen += adv;
            }
            None => {
                if !g.missing.contains(&c) {
                    g.missing.push(c);
                }
                pen += space;
            }
        }
    }
    Ok(g)
}
