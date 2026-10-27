//! M10b acceptance tests 25, 26, 28 (24 and 27 test crate internals, in `src/`).

use hc_geom::{containment_tree, BBox, Contour, Orientation, Point, Segment, EPS};
use hc_text::{text, Font, TextError, TextGeom};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn bbox(cs: &[Contour]) -> BBox {
    cs.iter().fold(BBox::empty(), |b, c| b.union(&c.bbox()))
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn hershey_hi() {
    let g = text("HI", Font::Hershey, 10.0, p(5.0, 5.0)).unwrap();
    assert_eq!(g.contours.len(), 4);
    assert!(g.missing.is_empty());
    for c in &g.contours {
        assert!(!c.closed);
        assert!(c.segments.iter().all(|s| matches!(s, Segment::Line { .. })));
    }
    let b = bbox(&g.contours);
    assert!(close(b.height(), 10.0, 1e-9), "{}", b.height());
    assert!(close(b.min.y, 5.0, 1e-9));
}

#[test]
fn hershey_o_stays_open() {
    let g = text("O", Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.contours.len(), 1);
    let c = &g.contours[0];
    assert!(!c.closed);
    assert!(c.start().approx_eq(c.end(), 1e-9));
}

#[test]
fn hershey_space_missing_and_advance() {
    let g = text(" ", Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g, TextGeom::default());

    let g = text("Hé", Font::Hershey, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.missing, vec!['é']);
    assert_eq!(
        g.contours,
        text("H", Font::Hershey, 10.0, p(0.0, 0.0))
            .unwrap()
            .contours
    );

    let one = text("H", Font::Hershey, 10.0, p(0.0, 0.0))
        .unwrap()
        .contours;
    let hh = text("HH", Font::Hershey, 10.0, p(0.0, 0.0))
        .unwrap()
        .contours;
    assert_eq!(hh.len(), 6);
    // H: bounds G..] = 22 units, cap height 21 units
    let adv = 22.0 * 10.0 / 21.0;
    for (a, b) in one.iter().zip(&hh[3..]) {
        for (sa, sb) in a.segments.iter().zip(&b.segments) {
            assert!(sb.start().approx_eq(sa.start() + p(adv, 0.0), 1e-12));
            assert!(sb.end().approx_eq(sa.end() + p(adv, 0.0), 1e-12));
        }
    }
}

#[test]
fn missing_chars_in_order_without_duplicates() {
    let g = text("éHé\nü", Font::Hershey, 5.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.missing, vec!['é', '\n', 'ü']);
    let g = text("a\nb", Font::Outline, 5.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.missing, vec!['\n']);
}

#[test]
fn outline_ho() {
    let g = text("HO", Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.contours.len(), 3);
    assert!(g.missing.is_empty());
    assert!(g.contours.iter().all(|c| c.closed));
    let h = &g.contours[0];
    let b = h.bbox();
    assert!(close(b.height(), 10.0, 1e-6), "{}", b.height());
    assert!(close(b.min.y, 0.0, 1e-6));
    for c in &g.contours {
        assert!(c.segments.iter().all(|s| s.length() >= EPS));
        assert!(c.is_closed_within(EPS));
        for w in c.segments.windows(2) {
            assert_eq!(w[0].end(), w[1].start());
        }
    }
    // O's counter sits under O's outline (the larger of the two)
    let tree = containment_tree(&g.contours);
    let (outer, inner) = if g.contours[1].signed_area().abs() > g.contours[2].signed_area().abs() {
        (1, 2)
    } else {
        (2, 1)
    };
    assert_eq!(tree[inner].parent, Some(outer));
    assert_eq!(tree[inner].depth, 1);
    assert_eq!(tree[0].parent, None);
    assert_eq!(g.contours[outer].orientation(), Some(Orientation::Ccw));
    assert_eq!(g.contours[inner].orientation(), Some(Orientation::Cw));
    assert_eq!(h.orientation(), Some(Orientation::Ccw));
}

#[test]
fn outline_missing_and_empty() {
    let g = text("中", Font::Outline, 10.0, p(0.0, 0.0)).unwrap();
    assert_eq!(g.missing, vec!['中']);
    assert!(g.contours.is_empty());
    assert_eq!(
        text("", Font::Outline, 10.0, p(0.0, 0.0)).unwrap(),
        TextGeom::default()
    );
}

#[test]
fn outline_is_placed_at_the_insertion_point_and_deterministic() {
    let a = text("Ab", Font::Outline, 7.0, p(0.0, 0.0)).unwrap();
    let b = text("Ab", Font::Outline, 7.0, p(0.0, 0.0)).unwrap();
    assert_eq!(a, b);
    let moved = text("Ab", Font::Outline, 7.0, p(10.0, -3.0)).unwrap();
    let ba = bbox(&a.contours);
    let bm = bbox(&moved.contours);
    assert!(bm.min.approx_eq(ba.min + p(10.0, -3.0), 1e-9));
}

#[test]
fn bad_height() {
    for font in [Font::Hershey, Font::Outline] {
        for h in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(text("H", font, h, p(0.0, 0.0)), Err(TextError::BadHeight));
        }
    }
    assert_eq!(
        TextError::BadHeight.to_string(),
        "Text height must be greater than 0"
    );
}
