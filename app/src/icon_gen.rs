//! M12 app icon: an original cutter-on-contour motif drawn with tiny-skia (generator is `#[ignore]`d, run by
//! `scripts/make-icons.sh`), plus a validity check of the committed assets.

use std::path::Path;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

const ICON_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon");
const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Sizes in `hobbycam.ico`, in directory order.
const ICO_SIZES: [u32; 4] = [16, 32, 48, 256];

/// Cubic Bézier factor for a quarter circle.
const K: f32 = 0.552_284_8;

fn paint(hex: u32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
        255,
    ));
    p.anti_alias = true;
    p
}

/// Closed rounded rectangle `[a, b]²` with corner radius `r` (1024-unit coordinates).
fn rounded_square(pb: &mut PathBuilder, a: f32, b: f32, r: f32) {
    let k = r * K;
    pb.move_to(a + r, a);
    pb.line_to(b - r, a);
    pb.cubic_to(b - r + k, a, b, a + r - k, b, a + r);
    pb.line_to(b, b - r);
    pb.cubic_to(b, b - r + k, b - r + k, b, b - r, b);
    pb.line_to(a + r, b);
    pb.cubic_to(a + r - k, b, a, b - r + k, a, b - r);
    pb.line_to(a, a + r);
    pb.cubic_to(a, a + r - k, a + r - k, a, a + r, a);
    pb.close();
}

/// The same rounded square as an open contour with a `gap` centred on the top edge (the tab).
fn tabbed_contour(pb: &mut PathBuilder, a: f32, b: f32, r: f32, gap: f32) {
    let k = r * K;
    let mid = (a + b) / 2.0;
    pb.move_to(mid + gap / 2.0, a);
    pb.line_to(b - r, a);
    pb.cubic_to(b - r + k, a, b, a + r - k, b, a + r);
    pb.line_to(b, b - r);
    pb.cubic_to(b, b - r + k, b - r + k, b, b - r, b);
    pb.line_to(a + r, b);
    pb.cubic_to(a + r - k, b, a, b - r + k, a, b - r);
    pb.line_to(a, a + r);
    pb.cubic_to(a, a + r - k, a + r - k, a, a + r, a);
    pb.line_to(mid - gap / 2.0, a);
}

/// Renders the icon at `size`×`size` from the 1024-unit vector description (no resampling).
fn render(size: u32) -> Pixmap {
    const SLATE: u32 = 0x2E4057;
    let mut pm = Pixmap::new(size, size).expect("pixmap");
    let t = Transform::from_scale(size as f32 / 1024.0, size as f32 / 1024.0);

    let mut pb = PathBuilder::new();
    rounded_square(&mut pb, 100.0, 924.0, 185.0);
    pm.fill_path(
        &pb.finish().unwrap(),
        &paint(SLATE),
        FillRule::Winding,
        t,
        None,
    );

    let (a, b, width) = (270.0, 754.0, 56.0);
    let mut pb = PathBuilder::new();
    // the round caps reach `width / 2` into the opening; 90 px stay visible
    tabbed_contour(&mut pb, a, b, 60.0, 90.0 + width);
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    pm.stroke_path(&pb.finish().unwrap(), &paint(0xF28C28), &stroke, t, None);

    let cutter = PathBuilder::from_circle(b, b, 110.0).unwrap();
    pm.fill_path(&cutter, &paint(0xFFFFFF), FillRule::Winding, t, None);
    let dot = PathBuilder::from_circle(b, b, 20.0).unwrap();
    pm.fill_path(&dot, &paint(SLATE), FillRule::Winding, t, None);
    pm
}

fn png(size: u32) -> Vec<u8> {
    render(size).encode_png().expect("png")
}

/// ICO with PNG-compressed entries (Vista+): 6-byte header, 16-byte directory entries, then the blobs.
fn ico(pngs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, blob) in pngs {
        let b = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[b, b, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += blob.len() as u32;
    }
    for (_, blob) in pngs {
        out.extend_from_slice(blob);
    }
    out
}

/// `(width, height)` from a PNG's IHDR chunk.
fn png_size(blob: &[u8]) -> (u32, u32) {
    assert!(blob.starts_with(PNG_SIG), "not a PNG");
    assert_eq!(&blob[12..16], b"IHDR");
    let be = |i: usize| u32::from_be_bytes(blob[i..i + 4].try_into().unwrap());
    (be(16), be(20))
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

#[test]
#[ignore = "generator: run scripts/make-icons.sh"]
fn make_icons() {
    let dir = std::env::var("HOBBYCAM_ICON_DIR").expect("set HOBBYCAM_ICON_DIR");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    for base in [16, 32, 128, 256, 512] {
        write(&format!("icon_{base}x{base}.png"), &png(base));
        write(&format!("icon_{base}x{base}@2x.png"), &png(2 * base));
    }
    write("hobbycam-1024.png", &png(1024));
    write("hobbycam-256.png", &png(256));
    let entries: Vec<_> = ICO_SIZES.iter().map(|&s| (s, png(s))).collect();
    write("hobbycam.ico", &ico(&entries));
}

/// Parses an ICO and checks it holds PNG entries of `ICO_SIZES`.
fn check_ico(b: &[u8]) {
    assert_eq!(le16(b, 0), 0, "reserved");
    assert_eq!(le16(b, 2), 1, "type icon");
    assert_eq!(le16(b, 4) as usize, ICO_SIZES.len(), "count");
    for (i, &size) in ICO_SIZES.iter().enumerate() {
        let e = 6 + 16 * i;
        let dim = |v: u8| if v == 0 { 256 } else { u32::from(v) };
        assert_eq!((dim(b[e]), dim(b[e + 1])), (size, size), "entry {i}");
        let (len, off) = (le32(b, e + 8) as usize, le32(b, e + 12) as usize);
        let blob = &b[off..off + len];
        assert_eq!(png_size(blob), (size, size), "entry {i} IHDR");
    }
}

#[test]
fn icon_assets_valid() {
    let dir = Path::new(ICON_DIR);
    check_ico(&std::fs::read(dir.join("hobbycam.ico")).unwrap());

    let icns = std::fs::read(dir.join("hobbycam.icns")).unwrap();
    assert_eq!(&icns[..4], b"icns");
    let len = u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize;
    assert_eq!(len, icns.len(), "icns length field");

    let icon =
        eframe::icon_data::from_png_bytes(&std::fs::read(dir.join("hobbycam-256.png")).unwrap())
            .expect("decodes");
    assert_eq!((icon.width, icon.height), (256, 256));
    let big = std::fs::read(dir.join("hobbycam-1024.png")).unwrap();
    assert_eq!(png_size(&big), (1024, 1024));
}

#[test]
fn ico_writer_round_trips() {
    let entries: Vec<_> = ICO_SIZES.iter().map(|&s| (s, png(s))).collect();
    check_ico(&ico(&entries));
}

#[test]
fn render_design_colours() {
    let pm = render(1024);
    let at = |x: u32, y: u32| pm.pixel(x, y).unwrap();
    assert_eq!(at(10, 10).alpha(), 0, "transparent outside the square");
    let slate = at(512, 512);
    assert_eq!(
        (slate.red(), slate.green(), slate.blue()),
        (0x2E, 0x40, 0x57)
    );
    let orange = at(270, 512);
    assert_eq!(
        (orange.red(), orange.green(), orange.blue()),
        (0xF2, 0x8C, 0x28)
    );
    for x in [512 - 44, 512, 512 + 44] {
        assert_eq!(
            at(x, 270).red(),
            0x2E,
            "90 px tab gap on the top edge at x={x}"
        );
    }
    assert_eq!(
        at(512 - 50, 270).red(),
        0xF2,
        "contour resumes after the gap"
    );
    let white = at(754 - 60, 754);
    assert_eq!((white.red(), white.green(), white.blue()), (255, 255, 255));
    assert_eq!(at(754, 754).red(), 0x2E, "slate dot in the cutter centre");
}
