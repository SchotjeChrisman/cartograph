mod common;

use common::*;
use graph::{DEPENDENCY_KINDS, Direction, EdgeKind, IndexerLock, Query, Reader, Writer};

const NOW: i64 = 1_700_000_000;

/// chain a0 -> a1 -> a2 -> a3 -> a4 in `a.rs`, plus `b.rs`: b0 -> a2 and a `contains` edge.
fn load_fixture(dir: &graph::GraphDir) {
    let lock = IndexerLock::acquire(dir).unwrap();
    let mut writer = Writer::open(dir).unwrap();
    let mut a = chain_file("a.rs", "a", 5);
    a.edges.push(edge(
        function_id("a.rs", "a0"),
        function_id("a.rs", "a1"),
        EdgeKind::Contains,
    ));
    let b = facts(
        "b.rs",
        vec![function("b0", 1)],
        vec![call(function_id("b.rs", "b0"), function_id("a.rs", "a2"))],
    );
    writer.load(&lock, vec![a, b], 10, NOW).unwrap();
}

fn names(nodes: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<_> = nodes.into_iter().collect();
    names.sort();
    names
}

#[test]
fn impact_walks_callers_up_to_the_depth() {
    let scratch = TestDir::new("impact");
    let dir = scratch.graph_dir();
    load_fixture(&dir);
    let reader = Reader::open(&dir).unwrap();

    let a3 = function_id("a.rs", "a3");
    let impact = reader.impact(a3, DEPENDENCY_KINDS, 10, 100).unwrap();
    assert_eq!(
        names(impact.reached.iter().map(|r| r.node.qualified_name.clone())),
        ["a0", "a1", "a2", "b0"]
    );
    assert_eq!(impact.total, 4);
    assert!(!impact.truncated);
    let depth_of = |name: &str| {
        impact
            .reached
            .iter()
            .find(|r| r.node.qualified_name == name)
            .unwrap()
            .depth
    };
    assert_eq!((depth_of("a2"), depth_of("a1"), depth_of("b0")), (1, 2, 2));

    let shallow = reader.impact(a3, DEPENDENCY_KINDS, 1, 100).unwrap();
    assert_eq!(shallow.total, 1);
    let cut = reader.impact(a3, DEPENDENCY_KINDS, 10, 2).unwrap();
    assert_eq!((cut.reached.len(), cut.total), (2, 4));
    assert_eq!(cut.reached[0].node.qualified_name, "a2", "nearest first");
}

#[test]
fn impact_respects_edge_kind_filters() {
    let scratch = TestDir::new("impact-kinds");
    let dir = scratch.graph_dir();
    load_fixture(&dir);
    let reader = Reader::open(&dir).unwrap();
    let contained = reader
        .impact(function_id("a.rs", "a1"), &[EdgeKind::Contains], 5, 10)
        .unwrap();
    assert_eq!(contained.reached.len(), 1);
    assert_eq!(contained.reached[0].node.qualified_name, "a0");
}

#[test]
fn path_finds_the_shortest_route_and_reports_direction() {
    let scratch = TestDir::new("path");
    let dir = scratch.graph_dir();
    load_fixture(&dir);
    let reader = Reader::open(&dir).unwrap();
    let (a0, a4, b0) = (
        function_id("a.rs", "a0"),
        function_id("a.rs", "a4"),
        function_id("b.rs", "b0"),
    );

    let forward = reader
        .path(a0, a4, Direction::Out, &[EdgeKind::Calls], 10)
        .unwrap()
        .unwrap();
    let route: Vec<_> = forward
        .iter()
        .map(|hop| hop.node.qualified_name.as_str())
        .collect();
    assert_eq!(route, ["a0", "a1", "a2", "a3", "a4"]);
    assert!(forward[0].via.is_none() && forward[1..].iter().all(|hop| hop.via.unwrap().forward));

    assert!(
        reader
            .path(a4, a0, Direction::Out, &[EdgeKind::Calls], 10)
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .path(a0, a4, Direction::Out, &[EdgeKind::Calls], 3)
            .unwrap()
            .is_none()
    );

    let across = reader
        .path(b0, a0, Direction::Both, &[EdgeKind::Calls], 10)
        .unwrap()
        .unwrap();
    assert_eq!(across.len(), 4);
    assert!(!across[3].via.unwrap().forward);
    assert_eq!(
        reader
            .path(a0, a0, Direction::Out, &[], 1)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn slice_stops_at_the_node_cap_and_says_so() {
    let scratch = TestDir::new("slice-cap");
    let dir = scratch.graph_dir();
    load_fixture(&dir);
    let reader = Reader::open(&dir).unwrap();
    let start = function_id("a.rs", "a0");

    let capped = reader
        .load_slice(start, Direction::Out, &[EdgeKind::Calls], 10, 3)
        .unwrap();
    assert!(capped.is_truncated());
    assert_eq!(capped.node_count(), 3);
    let full = reader
        .load_slice(start, Direction::Out, &[EdgeKind::Calls], 10, 100)
        .unwrap();
    assert!(!full.is_truncated());
    assert_eq!(full.node_count(), 5);
}
