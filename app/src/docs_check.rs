//! M11 accuracy guard: the user manual in `docs/src` stays in sync with the code (rule messages and severities,
//! sidebar names and keys, links, images, includes, the sample DXF).

use crate::state::AppState;
use crate::App;
use eframe::egui;
use hc_toolpath::{RuleId, Severity};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/src");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The 14 chapters, in SUMMARY order.
const PAGES: [&str; 14] = [
    "introduction.md",
    "install.md",
    "interface.md",
    "walkthrough.md",
    "drawing.md",
    "stock-origin.md",
    "tools.md",
    "operations.md",
    "tabs.md",
    "simulation.md",
    "sanity.md",
    "linuxcnc.md",
    "shortcuts.md",
    "limitations.md",
];

fn read(path: impl AsRef<Path>) -> String {
    let p = path.as_ref();
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn doc(name: &str) -> String {
    read(Path::new(DOCS).join(name))
}

/// Literal parts of each rule's message(s); the first one is in the `###` heading of `sanity.md`.
/// Exhaustive on purpose: a new RuleId does not compile until it is documented here and in the manual.
fn fragments(r: RuleId) -> &'static [&'static str] {
    use RuleId::*;
    match r {
        MissingTool => &["which is not in the project"],
        MissingItem => &["which does not exist"],
        DuplicateId => &["is used more than once"],
        DuplicateToolId => &["is embedded more than once"],
        IdCounter => &["is not below the id counter"],
        InvalidMachine => &[
            "Clearance Z must be greater than 0",
            "Safe Z must be at or above clearance Z",
            "Tool change Z must be at or above safe Z",
        ],
        // the heading is "Stock *width* must be …": "Stock " is the only literal it shares with the code
        InvalidStock => &[
            "Stock ",
            "must be greater than 0",
            "Stock position",
            "must be a finite number",
        ],
        InvalidValue => &["Invalid value:"],
        NoItems => &["No geometry assigned"],
        OpenContour => &["Needs closed contours, but got"],
        UnsupportedItem => &["in this operation"],
        ToolTooLarge => &["Tool too large for"],
        DrillBitSideCut => &["is a drill bit and cannot mill"],
        DepthBeyondFlutes => &["is deeper than the flute length"],
        SpindleOff => &["Spindle speed is 0 (S0)"],
        FeedVsTool => &["is far from the tool's"],
        StepoverHigh => &["may leave material between rings"],
        ThroughCut => &["into the spoilboard"],
        TabsNotThrough => &["Tabs are enabled but the cut does not go through the stock"],
        TabTooHigh => &["is not below the depth"],
        TabsReduced => &["tabs fit on"],
        TabsOnOpenContour => &["it is not closed"],
        NoTabsThroughCut => &["The part may come loose: through-cut without tabs"],
        TabOnCorner => &["sits on a corner and may be weak or misplaced"],
        DrillSize => &["drill Ø"],
        SharedEdge => &["the pocket treats them as one wall"],
        OutsideStock => &["outside the stock"],
        PocketUncut => &["are narrower than the tool and stay uncut"],
    }
}

/// `### ` headings under `## Rules` with the text up to the next `### ` / `## `.
fn rule_sections(md: &str) -> Vec<(String, String)> {
    let rules = md
        .split("\n## Rules")
        .nth(1)
        .expect("sanity.md has ## Rules");
    let rules = rules.split("\n## ").next().unwrap();
    rules
        .split("\n### ")
        .skip(1)
        .map(|s| {
            let (head, body) = s.split_once('\n').unwrap_or((s, ""));
            (head.to_string(), body.to_string())
        })
        .collect()
}

#[test]
fn docs_sanity_rules() {
    let distinct: HashSet<String> = RuleId::ALL.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(distinct.len(), 28);
    let code = read(Path::new(ROOT).join("crates/hc-toolpath/src/check.rs"))
        + &read(Path::new(ROOT).join("crates/hc-toolpath/src/lib.rs"));
    let sections = rule_sections(&doc("sanity.md"));
    assert_eq!(sections.len(), 28, "### headings under ## Rules");
    for r in RuleId::ALL {
        let f = fragments(r);
        for frag in f {
            assert!(code.contains(frag), "{r:?}: {frag:?} not in the code");
        }
        let hits: Vec<_> = sections.iter().filter(|(h, _)| h.contains(f[0])).collect();
        assert_eq!(hits.len(), 1, "{r:?}: headings containing {:?}", f[0]);
        let (_, body) = hits[0];
        let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let want = match r.severity() {
            Severity::Error => "**Error**",
            Severity::Warning => "**Warning**",
        };
        assert!(first.starts_with(want), "{r:?}: first line {first:?}");
        for frag in &f[1..] {
            assert!(body.contains(frag), "{r:?}: {frag:?} not in its section");
        }
    }
}

/// The sidebar entries of a fresh app (the context runs once first: formatting a shortcut needs its fonts).
fn sidebar_names() -> Vec<(&'static str, Option<String>)> {
    let ctx = egui::Context::default();
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    let app = App::new(AppState::new(None));
    crate::panels::sidebar_groups(&app, &ctx)
        .into_iter()
        .flatten()
        .map(|e| (e.name, e.shortcut))
        .collect()
}

#[test]
fn docs_shortcuts() {
    let md = doc("shortcuts.md");
    let rows: Vec<&str> = md.lines().filter(|l| l.starts_with('|')).collect();
    let row = |name: &str| -> Vec<&str> {
        let v: Vec<&str> = rows.iter().copied().filter(|l| l.contains(name)).collect();
        assert!(!v.is_empty(), "no table row for {name:?}");
        v
    };
    for (name, shortcut) in sidebar_names() {
        let rs = row(name);
        if let Some(k) = shortcut.as_deref() {
            if ["Esc", "L", "R", "C", "T", "O", "F", "Del"].contains(&k) {
                let key = format!("`{k}`");
                assert!(rs.iter().any(|l| l.contains(&key)), "{name}: {key}");
            }
        }
    }
    row("Save as…");
    assert!(row("Redo").iter().any(|l| l.contains("`Ctrl+Y`")));
}

/// Relative targets of `](…)` links/images and `{{#include …}}` in one page.
fn targets(md: &str) -> (Vec<String>, Vec<String>) {
    let mut links = Vec::new();
    for part in md.split("](").skip(1) {
        let t = part.split([')', ' ']).next().unwrap_or("");
        let t = t.split('#').next().unwrap_or("");
        let external = ["http:", "https:", "mailto:"]
            .iter()
            .any(|p| part.starts_with(p));
        if !external && !t.is_empty() {
            links.push(t.to_string());
        }
    }
    let includes = md
        .split("{{#include ")
        .skip(1)
        .map(|p| p.split("}}").next().unwrap().trim().to_string())
        .collect();
    (links, includes)
}

#[test]
fn docs_links() {
    let src = PathBuf::from(DOCS);
    let summary = doc("SUMMARY.md");
    let (entries, _) = targets(&summary);
    assert_eq!(entries, PAGES, "SUMMARY.md chapters");
    let mut referenced = HashSet::new();
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    pages.sort();
    for page in &pages {
        let (links, includes) = targets(&read(page));
        for t in links.iter().chain(&includes) {
            let p = src.join(t);
            assert!(p.exists(), "{}: {t} does not exist", page.display());
            if let Ok(c) = p.canonicalize() {
                referenced.insert(c);
            }
        }
    }
    for e in std::fs::read_dir(src.join("images")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "png") {
            assert!(
                referenced.contains(&p.canonicalize().unwrap()),
                "orphan image {}",
                p.display()
            );
        }
    }
    let dxf = |p: &str| std::fs::read(Path::new(ROOT).join(p)).unwrap();
    assert_eq!(
        dxf("docs/src/examples/bracket.dxf"),
        dxf("tests/fixtures/dxf/autocad2018_bracket_inch.dxf"),
        "docs/src/examples/bracket.dxf must be a byte copy of the fixture"
    );
}

#[test]
fn docs_help_menu_opens_manual() {
    use egui_kittest::kittest::Queryable;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, app: &mut App| app.show(ui),
            App::new(AppState::new(None)),
        );
    h.run();
    // Help comes right after Settings in the menu bar
    let x = |name: &str| h.get_by_label(name).rect().min.x;
    let (settings, help) = (x("Settings"), x("Help"));
    assert!(help > settings);
    assert!(crate::kittest::menu_q(&h, "Help", false).is_some());
    h.get_by_label("Help").click();
    h.run();
    h.get_by_label("User manual").click();
    h.step();
    let opened =
        h.output().platform_output.commands.iter().any(
            |c| matches!(c, egui::OutputCommand::OpenUrl(u) if u.url == crate::panels::DOCS_URL),
        );
    assert!(opened, "{:?}", h.output().platform_output.commands);
    // not in the sidebar
    assert!(sidebar_names()
        .iter()
        .all(|(n, _)| !n.contains("manual") && *n != "Help"));
}

#[test]
fn targets_parsing() {
    let (l, i) = targets(
        "![a](images/x.png) [b](tabs.md#edit) [c](https://x.y) [d](#top) {{#include ../../t.ngc}}",
    );
    assert_eq!(l, ["images/x.png", "tabs.md"]);
    assert_eq!(i, ["../../t.ngc"]);
    let s = rule_sections(
        "x\n## Rules\n\n### A\n**Error**\nz\n### B\n\n**Warning**\n## Other\n### C\n",
    );
    assert_eq!(s.len(), 2);
    assert_eq!(s[1].0, "B");
}

/// walkthrough.md steps 2, 3, 7 and 8 on the `bracket_clean()` project: the real blank, the "Center" snap of the
/// Ø76.2 circle as origin, no issues after the pick, two files `_1_T1` / `_2_T2`, header naming the picked point.
#[test]
fn docs_walkthrough_origin_and_export() {
    use crate::snap::SnapKind;
    use crate::state::tests::{bracket_clean, pocket_center};
    use crate::state::CanvasTool;
    use hc_project::WorkOrigin;
    let (mut s, _) = bracket_clean();
    // step 2: the real blank; step 3: hover the Ø76.2 circle's centre (10 px capture at ~1.3 px/mm) → Center snap
    let mut st = s.project.stock.clone();
    (st.min_x, st.min_y, st.width, st.height) = (17.0, 19.0, 220.0, 140.0);
    s.set_stock(st);
    s.set_tool(CanvasTool::SetOrigin);
    let center = pocket_center(&s);
    let snap = s
        .snap_at(center + hc_geom::Point::new(2.0, 1.0), 10.0 / 1.29)
        .expect("a snap near the centre");
    assert_eq!(snap.kind, SnapKind::Center);
    assert!(snap.p.distance(center) < 1e-9);
    s.canvas_click(snap.p, true);
    assert_eq!(s.tool, CanvasTool::Select);
    let WorkOrigin::Point(o) = s.project.stock.origin else {
        panic!("origin not picked: {:?}", s.project.stock.origin)
    };
    assert!(o.distance(center) < 1e-9);
    // the same project the shot_docs pictures use
    let (mut w, _) = bracket_clean();
    crate::state::tests::walkthrough_stock_origin(&mut w);
    assert_eq!(s.project.stock, w.project.stock);
    s.refresh_toolpaths();
    assert!(s.issues.is_empty(), "{:?}", s.issues);
    let files = hc_post::export(&s.project, "bracket").unwrap();
    let suffixes: Vec<&str> = files.iter().map(|f| f.suffix.as_str()).collect();
    assert_eq!(suffixes, ["_1_T1", "_2_T2"]);
    assert!(files[0].text.contains("XY0 = drawing point X"));
    // the header carries the export name (walkthrough step 8 never saves the project)
    assert!(files[0].text.starts_with("(bracket - file 1 of 2)\n"));
}
