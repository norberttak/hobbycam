//! Hershey Roman Simplex (`fonts/futural.jhf`, see `fonts/HERSHEY-NOTICE.txt`): JHF parser and glyph placement.

use hc_geom::{Contour, Point, Segment, EPS};
use std::sync::OnceLock;

const FUTURAL: &str = include_str!("../fonts/futural.jhf");

/// One JHF glyph in file units (Y down).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Glyph {
    pub left: i32,
    pub right: i32,
    pub strokes: Vec<Vec<(i32, i32)>>,
}

fn coord(b: u8) -> i32 {
    b as i32 - b'R' as i32
}

/// Count-driven JHF parser: cols 0–4 number, 5–7 vertex count incl. the bounds pair, then char pairs (" R" = pen up).
/// A glyph continues on following lines until 2·count characters are read.
pub(crate) fn parse(src: &str) -> Result<Vec<Glyph>, String> {
    let mut lines = src.lines().enumerate();
    let mut out = Vec::new();
    while let Some((no, line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let err = |m: &str| format!("line {}: {m}", no + 1);
        if !line.is_ascii() || line.len() < 8 {
            return Err(err("not a JHF glyph line"));
        }
        let count: usize = line[5..8]
            .trim()
            .parse()
            .map_err(|_| err("bad vertex count"))?;
        if count == 0 {
            return Err(err("vertex count 0"));
        }
        let mut data = line[8..].to_string();
        while data.len() < 2 * count {
            let (_, next) = lines.next().ok_or_else(|| err("glyph data cut short"))?;
            if !next.is_ascii() {
                return Err(err("not ASCII"));
            }
            data.push_str(next);
        }
        let b = &data.as_bytes()[..2 * count];
        let mut strokes = vec![Vec::new()];
        for pair in b[2..].chunks(2) {
            if pair == b" R" {
                strokes.push(Vec::new());
            } else {
                strokes
                    .last_mut()
                    .expect("one stroke")
                    .push((coord(pair[0]), coord(pair[1])));
            }
        }
        strokes.retain(|s| !s.is_empty());
        out.push(Glyph {
            left: coord(b[0]),
            right: coord(b[1]),
            strokes,
        });
    }
    Ok(out)
}

struct Font {
    /// Glyph k ↔ ASCII 32 + k.
    glyphs: Vec<Glyph>,
    /// `H` top and bottom (file Y, down); bottom = baseline.
    cap_top: i32,
    baseline: i32,
}

fn font() -> &'static Font {
    static F: OnceLock<Font> = OnceLock::new();
    F.get_or_init(|| {
        let glyphs = parse(FUTURAL).expect("bundled futural.jhf parses");
        let h = &glyphs[(b'H' - 32) as usize];
        let ys = || h.strokes.iter().flatten().map(|v| v.1);
        let (cap_top, baseline) = (ys().min().expect("H"), ys().max().expect("H"));
        Font {
            glyphs,
            cap_top,
            baseline,
        }
    })
}

fn scale(height: f64) -> f64 {
    let f = font();
    height / (f.baseline - f.cap_top) as f64
}

/// Space advance, mm.
pub(crate) fn space(height: f64) -> f64 {
    let g = &font().glyphs[0];
    (g.right - g.left) as f64 * scale(height)
}

/// Open strokes of `c` (ASCII 32–126 only) with its left bound at `origin.x` and baseline at `origin.y`.
pub(crate) fn glyph(c: char, height: f64, origin: Point) -> Option<(Vec<Contour>, f64)> {
    if !(' '..='~').contains(&c) {
        return None;
    }
    let f = font();
    let g = f.glyphs.get(c as usize - 32)?;
    let s = scale(height);
    let map = |(x, y): (i32, i32)| {
        Point::new(
            origin.x + (x - g.left) as f64 * s,
            origin.y + (f.baseline - y) as f64 * s,
        )
    };
    let contours = g
        .strokes
        .iter()
        .filter_map(|st| {
            let pts: Vec<Point> = st.iter().map(|&v| map(v)).collect();
            let segments: Vec<Segment> = pts
                .windows(2)
                .filter(|w| w[0].distance(w[1]) >= EPS)
                .map(|w| Segment::Line {
                    start: w[0],
                    end: w[1],
                })
                .collect();
            // always open, also when a stroke ends at its start (O): a stroke font is for Engrave only
            (!segments.is_empty()).then_some(Contour {
                segments,
                closed: false,
            })
        })
        .collect();
    Some((contours, (g.right - g.left) as f64 * s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M10b test 24.
    #[test]
    fn parses_the_bundled_font() {
        let g = parse(FUTURAL).unwrap();
        // ASCII 32–126 plus a 96th glyph (DEL, a filled box) that is never used
        assert_eq!(g.len(), 96);
        assert!((' '..='~').all(|c| glyph(c, 10.0, Point::new(0.0, 0.0)).is_some()));
        assert_eq!(glyph('\u{7f}', 10.0, Point::new(0.0, 0.0)), None);
        assert_eq!(g[(b'H' - 32) as usize].strokes.len(), 3);
        assert_eq!(g[(b'I' - 32) as usize].strokes.len(), 1);
        assert!(g[0].strokes.is_empty());
        assert_eq!((g[0].left, g[0].right), (-8, 8));
    }

    #[test]
    fn wrapped_glyph_parses_the_same() {
        let one = "12345  9G]KFK[ RYFY[ RKPYP\n";
        let two = "12345  9G]KFK[\n RYFY[ RKPYP\n";
        assert_eq!(parse(one).unwrap(), parse(two).unwrap());
        let g = &parse(two).unwrap()[0];
        assert_eq!((g.left, g.right), (-11, 11));
        assert_eq!(g.strokes[0], vec![(-7, -12), (-7, 9)]);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse("12345 xx\n").is_err());
        assert!(parse("12345  9G]KFK[\n").is_err());
        assert!(parse("12345  0\n").is_err());
        assert_eq!(parse("\n").unwrap(), vec![]);
    }
}
