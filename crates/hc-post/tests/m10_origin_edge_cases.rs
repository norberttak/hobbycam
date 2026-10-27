//! M10 verifier: picked work origin (WorkOrigin::Point) in real G-code.

use hc_geom::{shapes, Point};
use hc_post::*;
use hc_project::{Geom, OpKind, Project, Tool, ToolKind, Units, WorkOrigin};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn tool(kind: ToolKind, d: f64) -> Tool {
    Tool {
        id: 1,
        name: "V".into(),
        kind,
        diameter: d,
        flute_length: 20.0,
        flutes: 2,
        feed: 800.0,
        plunge_feed: 200.0,
        rpm: 18000,
        depth_per_pass: 1.0,
        stepover_pct: 40.0,
        comment: String::new(),
    }
}

/// Engrave of a drawn rect (10,20) 30 x 10 and a drill at (25,25); stock 0..60 x 0..50.
fn part() -> Project {
    let mut pr = Project::new();
    pr.stock.min_x = 0.0;
    pr.stock.min_y = 0.0;
    pr.stock.width = 60.0;
    pr.stock.height = 50.0;
    let ids = pr.add_drawn(vec![
        Geom::Contour(shapes::rect(p(10.0, 20.0), 30.0, 10.0, 0.0).unwrap()),
        Geom::Point(p(25.0, 25.0)),
    ]);
    pr.add_operation(
        "Engrave",
        OpKind::Engrave,
        &tool(ToolKind::FlatEndMill, 1.0),
        vec![ids[0]],
    );
    let mut d = tool(ToolKind::Drill, 3.0);
    d.id = 2;
    pr.add_operation("Drill", OpKind::Drill { peck: None }, &d, vec![ids[1]]);
    for o in &mut pr.operations {
        o.depth = 1.0;
    }
    pr
}

fn word(line: &str, letter: char) -> Option<f64> {
    line.split_whitespace()
        .skip(1)
        .find(|w| w.starts_with(letter))
        .map(|w| w[1..].parse().unwrap())
}

/// Every (X, Y) pair of the motion lines (G0/G1/G2/G3/G81/G83) that carry both words.
fn xy(text: &str) -> Vec<(f64, f64)> {
    text.lines()
        .filter(|l| !l.starts_with('('))
        .filter_map(|l| Some((word(l, 'X')?, word(l, 'Y')?)))
        .collect()
}

fn all_text(pr: &Project) -> String {
    export(pr, "t")
        .unwrap()
        .into_iter()
        .map(|f| f.text)
        .collect()
}

#[test]
fn origin_at_picked_corner_gives_x0_y0_there() {
    let mut pr = part();
    pr.stock.origin = WorkOrigin::Point(p(10.0, 20.0));
    let files = export(&pr, "t").unwrap();
    assert_eq!(files.len(), 2);
    let pts = xy(&files[0].text);
    assert!(pts.contains(&(0.0, 0.0)), "{}", files[0].text);
    assert!(pts.contains(&(30.0, 10.0)));
    let min_x = pts.iter().map(|q| q.0).fold(f64::INFINITY, f64::min);
    let min_y = pts.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    assert_eq!((min_x, min_y), (0.0, 0.0));
    assert!(files[0]
        .text
        .contains("(Units: mm. Z0 = top of stock. XY0 = drawing point X10 Y20)"));
    // the drill (25,25) → (15,5), in both its rapid and its cycle line
    assert!(files[1].text.contains("G0 X15 Y5"), "{}", files[1].text);
    assert!(files[1].text.contains("G81 X15 Y5 "), "{}", files[1].text);
    // Z untouched by the XY origin
    let z = |t: &str| -> Vec<f64> { t.lines().filter_map(|l| word(l, 'Z')).collect() };
    let mut base = part();
    base.stock.origin = WorkOrigin::DrawingOrigin;
    assert_eq!(z(&all_text(&base)), z(&all_text(&pr)));
}

#[test]
fn picked_origin_in_inch() {
    let mut pr = part();
    pr.units = Units::Inch;
    pr.stock.origin = WorkOrigin::Point(p(10.0, 20.0));
    let t = &export(&pr, "t").unwrap()[0].text;
    let pts = xy(t);
    assert!(pts.contains(&(0.0, 0.0)), "{t}");
    // 30 x 10 mm = 1.1811 x 0.3937 in
    assert!(pts.contains(&(1.1811, 0.3937)), "{t}");
    assert!(t.contains("XY0 = drawing point X0.3937 Y0.7874)"), "{t}");
    assert!(t.contains("G20"));
}

#[test]
fn origin_far_outside_the_stock_still_exports_and_shifts() {
    let mut pr = part();
    pr.stock.origin = WorkOrigin::Point(p(1000.0, -1000.0));
    let t = &export(&pr, "t").unwrap()[0].text;
    assert!(xy(t).contains(&(-990.0, 1020.0)), "{t}");
    assert!(t.contains("XY0 = drawing point X1000 Y-1000)"));
}

#[test]
fn negative_and_fractional_origin_header() {
    let mut pr = part();
    pr.stock.origin = WorkOrigin::Point(p(-2.5, 0.0004));
    let t = &export(&pr, "t").unwrap()[0].text;
    // 3 decimals in mm; a value that rounds to zero is "0", never "-0"
    assert!(t.contains("XY0 = drawing point X-2.5 Y0)"), "{t}");
    pr.stock.origin = WorkOrigin::Point(p(-0.0004, 0.0));
    let t = &export(&pr, "t").unwrap()[0].text;
    assert!(t.contains("XY0 = drawing point X0 Y0)"), "{t}");
    for l in t.lines().filter(|l| l.starts_with('(')) {
        assert_eq!(l.matches('(').count(), 1, "{l}");
        assert_eq!(l.matches(')').count(), 1, "{l}");
    }
}

#[test]
fn non_finite_picked_origin_is_never_exported() {
    for o in [
        p(f64::NAN, 0.0),
        p(0.0, f64::INFINITY),
        p(f64::NEG_INFINITY, 1.0),
    ] {
        let mut pr = part();
        pr.stock.origin = WorkOrigin::Point(o);
        let r = export(&pr, "t");
        assert!(r.is_err(), "{o:?}: {r:?}");
    }
}

#[test]
fn point_at_drawing_origin_equals_drawing_origin_moves() {
    let mut a = part();
    a.stock.origin = WorkOrigin::DrawingOrigin;
    let mut b = part();
    b.stock.origin = WorkOrigin::Point(p(0.0, 0.0));
    let (fa, fb) = (export(&a, "t").unwrap(), export(&b, "t").unwrap());
    for (x, y) in fa.iter().zip(&fb) {
        let strip = |t: &str| -> Vec<String> {
            t.lines()
                .filter(|l| !l.starts_with("(Units"))
                .map(String::from)
                .collect()
        };
        assert_eq!(strip(&x.text), strip(&y.text));
    }
}
