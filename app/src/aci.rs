//! AutoCAD Color Index → screen colour (standard colours 1–9 only).

use eframe::egui::{Color32, Visuals};

/// 7 is white on dark / black on light; anything outside 1–9 uses the theme's text colour.
pub fn aci_rgb(index: i16, dark: bool) -> Color32 {
    match index {
        1 => Color32::from_rgb(255, 0, 0),
        2 => Color32::from_rgb(255, 255, 0),
        3 => Color32::from_rgb(0, 255, 0),
        4 => Color32::from_rgb(0, 255, 255),
        5 => Color32::from_rgb(0, 0, 255),
        6 => Color32::from_rgb(255, 0, 255),
        7 if dark => Color32::WHITE,
        7 => Color32::BLACK,
        8 => Color32::from_rgb(128, 128, 128),
        9 => Color32::from_rgb(192, 192, 192),
        _ if dark => Visuals::dark().text_color(),
        _ => Visuals::light().text_color(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t10_aci() {
        let rgb = |i| aci_rgb(i, true);
        assert_eq!(rgb(1), Color32::from_rgb(255, 0, 0));
        assert_eq!(rgb(2), Color32::from_rgb(255, 255, 0));
        assert_eq!(rgb(3), Color32::from_rgb(0, 255, 0));
        assert_eq!(rgb(4), Color32::from_rgb(0, 255, 255));
        assert_eq!(rgb(5), Color32::from_rgb(0, 0, 255));
        assert_eq!(rgb(6), Color32::from_rgb(255, 0, 255));
        assert_eq!(aci_rgb(7, true), Color32::WHITE);
        assert_eq!(aci_rgb(7, false), Color32::BLACK);
        assert_eq!(rgb(8), Color32::from_rgb(128, 128, 128));
        assert_eq!(rgb(9), Color32::from_rgb(192, 192, 192));
        for i in [0, 10, 256, -1] {
            assert_eq!(aci_rgb(i, true), Visuals::dark().text_color());
            assert_eq!(aci_rgb(i, false), Visuals::light().text_color());
        }
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn extreme_indices_fall_back_to_text_colour() {
        for i in [i16::MIN, -7, 11, 255, 257, i16::MAX] {
            assert_eq!(aci_rgb(i, true), Visuals::dark().text_color(), "{i}");
            assert_eq!(aci_rgb(i, false), Visuals::light().text_color(), "{i}");
        }
        // 1-6, 8, 9 do not depend on the theme; 7 does
        for i in [1, 2, 3, 4, 5, 6, 8, 9] {
            assert_eq!(aci_rgb(i, true), aci_rgb(i, false), "{i}");
        }
        assert_ne!(aci_rgb(7, true), aci_rgb(7, false));
    }
}
