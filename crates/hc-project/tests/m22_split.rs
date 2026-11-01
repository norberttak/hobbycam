//! M22 P1: `Project::split_item`.
use hc_geom::{Contour, Point, Segment};
use hc_project::*;

fn geom(x: f64) -> Geom {
    Geom::Contour(Contour {
        segments: vec![Segment::Line {
            start: Point::new(x, 0.0),
            end: Point::new(x + 1.0, 0.0),
        }],
        closed: false,
    })
}

fn op(id: u64, items: Vec<u64>) -> Operation {
    Operation {
        id,
        name: format!("op{id}"),
        enabled: true,
        tool: 1,
        items,
        kind: OpKind::Engrave,
        depth: 1.0,
        depth_per_pass: 1.0,
        feed: 100.0,
        plunge_feed: 50.0,
        rpm: 1000,
        tabs: Tabs::default(),
    }
}

#[test]
fn p1_split_item() {
    let mut p = Project::new();
    for (id, x, layer) in [(1, 0.0, "A"), (2, 5.0, "B")] {
        p.items.push(Item {
            id,
            layer: layer.into(),
            source: Source::Drawn,
            geom: geom(x),
        });
    }
    p.next_id = 3;
    p.operations = vec![op(10, vec![1, 2]), op(11, vec![2])];
    let n = p.split_item(1, geom(9.0)).unwrap();
    assert_eq!(n, 3);
    assert_eq!(p.next_id, 4);
    let ids: Vec<u64> = p.items.iter().map(|i| i.id).collect();
    assert_eq!(ids, [1, 3, 2]);
    assert_eq!(p.items[1].layer, "A");
    assert_eq!(p.items[1].source, Source::Drawn);
    assert_eq!(p.items[1].geom, geom(9.0));
    assert_eq!(p.operations[0].items, [1, 3, 2]);
    assert_eq!(p.operations[1].items, [2]);
    let before = p.clone();
    assert!(p.split_item(99, geom(0.0)).is_none());
    assert_eq!(p, before);
}
