//! M10b verifier: every printable ASCII glyph of both fonts, heights, placement, Unicode input, bundled font files.

use hc_geom::{containment_tree, BBox, Contour, Orientation, Point, Segment, EPS};
use hc_text::{text, Font, TextError};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn bbox(cs: &[Contour]) -> BBox {
    cs.iter().fold(BBox::empty(), |b, c| b.union(&c.bbox()))
}

fn printable() -> impl Iterator<Item = char> {
    ('!'..='~').filter(|c| *c != ' ')
}

fn finite(q: Point) -> bool {
    q.x.is_finite() && q.y.is_finite()
}

/// Non-adjacent segment pairs of one closed contour that touch or cross.
fn self_hits(c: &Contour) -> Vec<(usize, usize, Point)> {
    let n = c.segments.len();
    let mut out = vec![];
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            for q in c.segments[i].intersections(&c.segments[j]) {
                out.push((i, j, q));
            }
        }
    }
    out
}

fn any_cross(a: &Contour, b: &Contour) -> Option<Point> {
    if a.bbox().union(&b.bbox()).width() > a.bbox().width() + b.bbox().width() + 1e-9 {
        return None;
    }
    a.segments.iter().find_map(|s| {
        b.segments
            .iter()
            .find_map(|t| s.intersections(t).into_iter().next())
    })
}

// ---------------------------------------------------------------- Hershey

#[test]
fn hershey_every_printable_glyph_is_finite_open_and_without_zero_segments() {
    for c in printable() {
        let g = text(&c.to_string(), Font::Hershey, 10.0, p(3.0, -2.0)).unwrap();
        assert!(g.missing.is_empty(), "{c}");
        assert!(!g.contours.is_empty(), "{c} has no strokes");
        for k in &g.contours {
            assert!(!k.closed, "{c}");
            for s in &k.segments {
                assert!(matches!(s, Segment::Line { .. }), "{c}");
                assert!(finite(s.start()) && finite(s.end()), "{c}");
                assert!(s.length() >= EPS, "{c}: zero-length segment");
            }
            for w in k.segments.windows(2) {
                assert_eq!(w[0].end(), w[1].start(), "{c}: stroke not connected");
            }
        }
        let b = bbox(&g.contours);
        assert!(finite(b.min) && finite(b.max), "{c}");
        // nothing far outside the em box (descenders ~ 0.35 h below, accents ~ 0.3 h above)
        assert!(b.min.y > -2.0 - 6.0 && b.max.y < -2.0 + 16.0, "{c}: {b:?}");
    }
}

#[test]
fn hershey_l_is_upright_and_not_mirrored() {
    // L: stem at the left, foot on the baseline going right
    let g = text("L", Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
    let b = bbox(&g.contours);
    assert!(b.min.y.abs() < 1e-9);
    let foot = g
        .contours
        .iter()
        .flat_map(|c| &c.segments)
        .find(|s| s.start().y.abs() < 1e-9 && s.end().y.abs() < 1e-9)
        .expect("a foot on the baseline");
    assert!(foot.start().x.min(foot.end().x) - b.min.x < 1e-9);
    assert!(foot.length() > 0.5 * b.width());
}

#[test]
fn hershey_multi_part_glyphs_have_separate_strokes() {
    for (c, n) in [
        ('i', 2),
        ('j', 2),
        ('!', 2),
        ('?', 2),
        (':', 2),
        (';', 2),
        ('"', 2),
        ('=', 2),
    ] {
        let g = text(&c.to_string(), Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
        assert!(g.contours.len() >= n, "{c}: {}", g.contours.len());
    }
}

// ---------------------------------------------------------------- Outline

#[test]
fn outline_every_printable_glyph_is_closed_exactly_and_finite() {
    for c in printable() {
        let g = text(&c.to_string(), Font::Outline, 10.0, p(3.0, -2.0)).unwrap();
        assert!(g.missing.is_empty(), "{c}");
        assert!(!g.contours.is_empty(), "{c} has no outline");
        for k in &g.contours {
            assert!(k.closed, "{c}");
            assert!(k.segments.len() >= 3, "{c}: {} segments", k.segments.len());
            // exact closure and exact joints, as the offsetter and the tree expect
            assert_eq!(k.end(), k.start(), "{c}: not exactly closed");
            for w in k.segments.windows(2) {
                assert_eq!(w[0].end(), w[1].start(), "{c}");
            }
            for s in &k.segments {
                assert!(finite(s.start()) && finite(s.end()), "{c}");
                assert!(s.length() >= EPS, "{c}: zero-length segment");
            }
            assert!(k.orientation().is_some(), "{c}: degenerate area");
        }
    }
}

#[test]
fn outline_no_self_intersections_and_no_crossing_contours() {
    for c in printable() {
        let g = text(&c.to_string(), Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
        for (k, a) in g.contours.iter().enumerate() {
            let hits = self_hits(a);
            assert!(hits.is_empty(), "{c} contour {k}: {:?}", &hits[..1]);
            for b in &g.contours[k + 1..] {
                assert_eq!(any_cross(a, b), None, "{c}: two contours cross");
            }
        }
    }
}

#[test]
fn outline_orientation_follows_nesting_for_every_glyph() {
    for c in printable() {
        let g = text(&c.to_string(), Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
        let tree = containment_tree(&g.contours);
        for n in &tree {
            let want = if n.depth % 2 == 0 {
                Orientation::Ccw
            } else {
                Orientation::Cw
            };
            assert_eq!(g.contours[n.contour].orientation(), Some(want), "{c}");
            // a counter lies wholly inside its outer contour
            if let Some(parent) = n.parent {
                assert!(g.contours[parent]
                    .bbox()
                    .contains_bbox(&g.contours[n.contour].bbox()));
            }
        }
    }
}

fn holes_and_outers(c: char) -> (usize, usize) {
    let g = text(&c.to_string(), Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    let cw = g
        .contours
        .iter()
        .filter(|k| k.orientation() == Some(Orientation::Cw))
        .count();
    (cw, g.contours.len() - cw)
}

#[test]
fn outline_counters_are_cw_holes() {
    // exact counts where the Ubuntu design is unambiguous
    for (c, holes) in [
        ('A', 1),
        ('B', 2),
        ('D', 1),
        ('O', 1),
        ('P', 1),
        ('Q', 1),
        ('R', 1),
        ('a', 1),
        ('b', 1),
        ('d', 1),
        ('e', 1),
        ('o', 1),
        ('p', 1),
        ('q', 1),
        ('0', 1),
        ('4', 1),
        ('6', 1),
        ('8', 2),
        ('9', 1),
        ('%', 2),
    ] {
        assert_eq!(holes_and_outers(c).0, holes, "{c}");
    }
    for c in ['g', '@', '&'] {
        assert!(holes_and_outers(c).0 >= 1, "{c}");
    }
    // no counter
    for c in ['H', 'I', 'L', 'x', '1', '-'] {
        assert_eq!(holes_and_outers(c).0, 0, "{c}");
    }
}

#[test]
fn outline_multi_part_glyphs_have_several_outer_contours() {
    for (c, outers) in [
        ('i', 2),
        ('j', 2),
        ('!', 2),
        ('?', 2),
        (':', 2),
        (';', 2),
        ('"', 2),
        ('=', 2),
        ('%', 3),
    ] {
        assert_eq!(holes_and_outers(c).1, outers, "{c}");
    }
}

#[test]
fn outline_l_is_upright_and_not_mirrored() {
    let g = text("L", Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    let b = bbox(&g.contours);
    let inside = |q: Point| g.contours.iter().filter(|c| c.contains_point(q)).count() % 2 == 1;
    // stem top-left is ink, top-right is not; the foot reaches the bottom right
    assert!(inside(p(b.min.x + 0.2, b.max.y - 0.2)));
    assert!(!inside(p(b.max.x - 0.2, b.max.y - 0.2)));
    assert!(inside(p(b.max.x - 0.2, b.min.y + 0.2)));
}

#[test]
fn adjacent_outline_glyphs_do_not_overlap() {
    // no kerning: a pair whose outlines cross breaks the containment tree and a Contour Outside gouges the neighbour.
    // Letters, digits and space never touch; some punctuation pairs do (see the second assert), a known no-kerning limit.
    let overlaps = |chars: &[char]| {
        let single: Vec<usize> = chars
            .iter()
            .map(|c| {
                text(&c.to_string(), Font::Outline, 10.0, p(0.0, 0.0))
                    .unwrap()
                    .contours
                    .len()
            })
            .collect();
        let mut bad = vec![];
        for (ia, &a) in chars.iter().enumerate() {
            for &b in chars {
                let g = text(&format!("{a}{b}"), Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
                let (left, right) = g.contours.split_at(single[ia]);
                if bbox(left).max.x < bbox(right).min.x {
                    continue;
                }
                if left
                    .iter()
                    .any(|l| right.iter().any(|r| any_cross(l, r).is_some()))
                {
                    bad.push(format!("{a}{b}"));
                }
            }
        }
        bad
    };
    let alnum: Vec<char> = printable().filter(|c| c.is_ascii_alphanumeric()).collect();
    assert_eq!(overlaps(&alnum), Vec::<String>::new());
    let all: Vec<char> = printable().collect();
    let bad = overlaps(&all);
    assert!(bad.contains(&"__".to_string()), "{bad:?}");
    assert!(
        bad.iter()
            .all(|s| s.chars().any(|c| "()[]{}/\\_".contains(c))),
        "{bad:?}"
    );
}

// ---------------------------------------------------------------- height and placement

fn h_height(font: Font, h: f64) -> (f64, f64) {
    let g = text("H", font, h, p(0.0, 0.0)).unwrap();
    let b = bbox(&g.contours);
    (b.height(), b.min.y)
}

#[test]
fn cap_height_holds_from_half_a_mm_to_half_a_metre() {
    for font in [Font::Hershey, Font::Outline] {
        for h in [0.5, 1.0, 10.0, 25.4, 500.0] {
            let (hh, y0) = h_height(font, h);
            assert!((hh - h).abs() <= 1e-9 * h.max(1.0), "{font:?} {h}: {hh}");
            assert!(y0.abs() <= 1e-9 * h.max(1.0), "{font:?} {h}: {y0}");
        }
    }
}

#[test]
fn text_scales_linearly_with_height() {
    for font in [Font::Hershey, Font::Outline] {
        let s = "Ag%";
        let a = bbox(&text(s, font, 10.0, p(0.0, 0.0)).unwrap().contours);
        let b = bbox(&text(s, font, 500.0, p(0.0, 0.0)).unwrap().contours);
        // flattening adds more pieces at 500 mm, so allow the tolerance band
        assert!(
            (b.width() - 50.0 * a.width()).abs() < 50.0 * 0.02,
            "{font:?}"
        );
        assert!(
            (b.height() - 50.0 * a.height()).abs() < 50.0 * 0.02,
            "{font:?}"
        );
    }
}

#[test]
fn tiny_heights_still_give_usable_geometry() {
    // 0.5 mm outline text: every segment still longer than EPS and every contour still has area
    let g = text("Hello @%&", Font::Outline, 0.5, p(0.0, 0.0)).unwrap();
    for c in &g.contours {
        assert!(c.orientation().is_some());
        assert!(c.segments.iter().all(|s| s.length() >= EPS));
    }
    // an absurdly small but positive height is accepted, not a panic
    for font in [Font::Hershey, Font::Outline] {
        let g = text("H", font, 1e-6, p(0.0, 0.0)).unwrap();
        assert!(bbox(&g.contours).height() <= 1e-6 + 1e-12);
    }
}

#[test]
fn large_outline_text_respects_the_flatten_tolerance_cap() {
    // n is clamped to 64 pieces per curve: at 500 mm cap height, report how far chords sag (no assert on the cap itself)
    let g = text("O", Font::Outline, 500.0, p(0.0, 0.0)).unwrap();
    let outer = g
        .contours
        .iter()
        .max_by(|a, b| a.signed_area().abs().total_cmp(&b.signed_area().abs()))
        .unwrap();
    let longest = outer
        .segments
        .iter()
        .map(|s| s.length())
        .fold(0.0, f64::max);
    // an O of 500 mm with 0.01 mm tolerance needs chords < ~2*sqrt(2*R*tol) ~ 2*sqrt(2*250*0.01) ~ 4.5 mm
    assert!(longest < 10.0, "longest chord {longest} mm");
}

#[test]
fn bad_heights_rejected_both_fonts() {
    for font in [Font::Hershey, Font::Outline] {
        for h in [
            0.0,
            -0.0,
            -1e-9,
            -500.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(
                text("H", font, h, p(0.0, 0.0)),
                Err(TextError::BadHeight),
                "{h}"
            );
        }
        // empty text is still validated
        assert_eq!(text("", font, 0.0, p(0.0, 0.0)), Err(TextError::BadHeight));
    }
}

#[test]
fn non_finite_insertion_point_gives_no_nan_geometry() {
    // the app only passes snapped or form values (finite); for the library: an error or no NaN, never NaN contours
    for font in [Font::Hershey, Font::Outline] {
        if let Ok(g) = text("H", font, 10.0, p(f64::NAN, 0.0)) {
            let nan = g
                .contours
                .iter()
                .flat_map(|c| &c.segments)
                .any(|s| !finite(s.start()) || !finite(s.end()));
            assert!(!nan, "{font:?}: NaN geometry from a NaN insertion point");
        }
    }
}

#[test]
fn far_from_origin_placement_is_exact_translation() {
    for font in [Font::Hershey, Font::Outline] {
        let a = text("B8", font, 10.0, p(0.0, 0.0)).unwrap();
        let b = text("B8", font, 10.0, p(1000.0, -2000.0)).unwrap();
        assert_eq!(a.contours.len(), b.contours.len());
        for (x, y) in a.contours.iter().zip(&b.contours) {
            for (s, t) in x.segments.iter().zip(&y.segments) {
                assert!(t.start().approx_eq(s.start() + p(1000.0, -2000.0), 1e-9));
            }
        }
    }
}

// ---------------------------------------------------------------- Unicode input

#[test]
fn unicode_input_never_panics_and_reports_missing() {
    let inputs = [
        "Árvíztűrő tükörfúrógép",
        "naïve café",
        "e\u{301}",    // combining acute
        "😀 smile 👍🏽", // emoji + skin tone modifier
        "中文字",
        "שלום",           // RTL Hebrew
        "مرحبا",          // RTL Arabic
        "\u{200F}ab",     // RLM
        "a\tb\nc\r\u{0}", // controls
        "\u{FEFF}x",      // BOM
        "\u{FFFD}",
        "\u{10FFFF}",
        "a\u{0}b",
    ];
    for s in inputs {
        for font in [Font::Hershey, Font::Outline] {
            let g = text(s, font, 10.0, p(0.0, 0.0)).unwrap();
            let mut seen = vec![];
            for c in &g.missing {
                assert!(!seen.contains(c), "duplicate {c:?} in {s:?}");
                seen.push(*c);
                assert!(s.contains(*c));
            }
            for k in &g.contours {
                assert!(k
                    .segments
                    .iter()
                    .all(|x| finite(x.start()) && finite(x.end())));
            }
        }
    }
    // Hershey is ASCII only: every non-ASCII character is missing
    let g = text("naïve", Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.missing, vec!['ï']);
    // controls are missing in both fonts, in text order
    for font in [Font::Hershey, Font::Outline] {
        let g = text("a\tb\nc", font, 10.0, p(0.0, 0.0)).unwrap();
        assert_eq!(g.missing, vec!['\t', '\n'], "{font:?}");
    }
}

#[test]
fn missing_char_advances_by_one_space() {
    for font in [Font::Hershey, Font::Outline] {
        let sp = text("H H", font, 10.0, p(0.0, 0.0)).unwrap();
        let miss = text("H\u{4E2D}H", font, 10.0, p(0.0, 0.0)).unwrap();
        assert_eq!(miss.missing, vec!['\u{4E2D}']);
        assert_eq!(sp.contours, miss.contours, "{font:?}");
    }
}

#[test]
fn outline_accented_latin_is_supported_with_separate_accent() {
    // Ubuntu covers Latin-1/Latin Extended-A: Hungarian text must not be reported missing
    let g = text("áéíóöőúüű ÁÉŐŰ", Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    assert!(g.missing.is_empty(), "{:?}", g.missing);
    // é = e (outer + counter) + accent (another outer); accent above the x-height
    let e = text("é", Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(holes_and_outers('é'), (1, 2));
    for k in &e.contours {
        assert!(self_hits(k).is_empty());
    }
}

#[test]
fn spaces_only_give_no_contours_and_no_missing() {
    for font in [Font::Hershey, Font::Outline] {
        let g = text("   ", font, 10.0, p(0.0, 0.0)).unwrap();
        assert!(g.contours.is_empty() && g.missing.is_empty(), "{font:?}");
    }
}

// ---------------------------------------------------------------- bundled files

#[test]
fn bundled_font_files_and_notices_are_present() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts");
    let jhf = std::fs::read_to_string(format!("{dir}/futural.jhf")).unwrap();
    assert_eq!(jhf.lines().count(), 96);
    let notice = std::fs::read_to_string(format!("{dir}/HERSHEY-NOTICE.txt")).unwrap();
    assert!(notice.contains("Hershey"));
    assert!(notice.contains("James Hurt"));
    let readme = std::fs::read_to_string(format!("{dir}/README.md")).unwrap();
    for h in [
        "4958a1d06ea04709feb530deaab53d1efa1285d0d3e92a4a3edd74d279871ced",
        "6ffb639e2d2799e987cec4f913473d4119e4270b65c13ceafe1cd502f904d855",
        "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70",
    ] {
        assert!(readme.contains(h), "{h}");
    }
    let top =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    assert!(top.contains("Dr. A. V. Hershey") && top.contains("James Hurt, Cognition, Inc."));
    assert!(top.contains("Ubuntu Font Licence 1.0"));
    // the Ubuntu bytes come from the pinned crate (its SHA-256 is checked by hand against fonts/README.md)
    assert!(epaint_default_fonts::UBUNTU_LIGHT.starts_with(&[0, 1, 0, 0]));
}
