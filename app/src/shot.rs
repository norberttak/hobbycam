//! Offscreen screenshots for visual review (no screen-recording permission needed).
//! Run: HOBBYCAM_SHOT_DIR=/some/dir cargo test -p hobbycam shot -- --ignored

use crate::state::{self, AppState};
use crate::App;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn save(h: &mut Harness<'_, App>, name: &str) {
    let dir = std::env::var("HOBBYCAM_SHOT_DIR").expect("set HOBBYCAM_SHOT_DIR");
    let img = h.render().expect("render");
    img.save(format!("{dir}/{name}.png")).expect("save png");
}

#[test]
#[ignore]
fn shot_bracket() {
    let mut s = AppState::new(None);
    s.open_cli(&state::tests::bracket_path());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.show(ui), App::new(s));
    h.run();
    save(&mut h, "bracket");
    h.get_by_label("File").click();
    h.run();
    save(&mut h, "file_menu");
}
