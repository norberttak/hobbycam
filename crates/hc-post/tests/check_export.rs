//! M8 edge cases (tester): export with mixed errors and warnings, warnings-only export, Blocked text.

use hc_geom::{Contour, Point};
use hc_post::{export, PostError};
use hc_project::{Geom, Item, OpKind, Project, Source, Tool, ToolKind};
use hc_toolpath::{check, generate, RuleId, Severity};

fn tool(id: u32, d: f64) -> Tool {
    Tool {
        id,
        name: format!("T{id}"),
        kind: ToolKind::FlatEndMill,
        diameter: d,
        flute_length: 20.0,
        flutes: 2,
        feed: 600.0,
        plunge_feed: 150.0,
        rpm: 12000,
        depth_per_pass: 1.0,
        stepover_pct: 40.0,
        comment: String::new(),
    }
}

/// Engrave on a 20 mm square (op 1) and a Pocket on it (op 2), both clean.
fn job() -> (Project, u64, u64) {
    let mut pr = Project::new();
    let sq = pr.next_id;
    pr.next_id += 1;
    let v = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    pr.items.push(Item {
        id: sq,
        layer: "0".into(),
        source: Source::Drawn,
        geom: Geom::Contour(Contour::from_bulge_vertices(
            &v.map(|(x, y)| (Point::new(x, y), 0.0)),
            true,
        )),
    });
    let e = pr.add_operation("Engrave 1", OpKind::Engrave, &tool(1, 3.0), vec![sq]);
    let pk = pr.add_operation(
        "Pocket 1",
        OpKind::Pocket { stepover_pct: 0.0 },
        &tool(1, 3.0),
        vec![sq],
    );
    (pr, e, pk)
}

#[test]
fn clean_job_exports() {
    let (pr, _, _) = job();
    assert_eq!(check(&pr, &generate(&pr)), []);
    assert_eq!(export(&pr, "x").unwrap().len(), 1);
}

#[test]
fn mixed_errors_and_warnings_block_with_errors_only() {
    let (mut pr, e, pk) = job();
    pr.operations[0].rpm = 0; // warning on op 1
    pr.operations[1].depth = 25.0; // DepthBeyondFlutes (error) + ThroughCut (warning)
    pr.operations[0].items.push(999); // MissingItem (error) on op 1
    let all = check(&pr, &generate(&pr));
    assert!(all.iter().any(|i| i.severity == Severity::Warning));
    match export(&pr, "x") {
        Err(PostError::Blocked(v)) => {
            let got: Vec<_> = v.iter().map(|i| (i.rule, i.op)).collect();
            assert_eq!(
                got,
                [
                    (RuleId::MissingItem, Some(e)),
                    (RuleId::DepthBeyondFlutes, Some(pk))
                ]
            );
            assert!(v.iter().all(|i| i.severity == Severity::Error));
            let text = PostError::Blocked(v.clone()).to_string();
            assert!(
                text.starts_with("Cannot export: 2 errors. First: "),
                "{text}"
            );
            assert!(text.ends_with("Refers to a deleted item (id 999), which does not exist"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn many_warnings_still_export() {
    let (mut pr, _, _) = job();
    pr.operations[0].rpm = 0; // SpindleOff
    pr.operations[1].feed = 600.0 * 25.4; // FeedVsTool
    pr.operations[1].kind = OpKind::Pocket { stepover_pct: 80.0 }; // StepoverHigh
    pr.stock.min_x = 5.0; // OutsideStock
    let v = check(&pr, &generate(&pr));
    assert!(v.len() >= 4, "{v:#?}");
    assert!(v.iter().all(|i| i.severity == Severity::Warning), "{v:#?}");
    let files = export(&pr, "x").unwrap();
    assert!(files[0].text.contains("S0 M3\n"));
    assert!(files[0].text.contains("F15240"));
}

#[test]
fn disabled_broken_op_does_not_block() {
    let (mut pr, _, _) = job();
    pr.operations[1].tool = 9;
    assert!(matches!(export(&pr, "x"), Err(PostError::Blocked(_))));
    pr.operations[1].enabled = false;
    assert!(export(&pr, "x").is_ok());
}
