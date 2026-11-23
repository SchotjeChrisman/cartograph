mod common;

use common::*;
use graph::{
    Component, ComponentId, ContentHash, Description, Direction, EdgeKind, Error, FileStatus,
    IndexerLock, NodeKind, Provenance, Query, Reader, SymbolId, Writer,
};

const NOW: i64 = 1_700_000_000;

fn load(dir: &graph::GraphDir, files: Vec<graph::FileFacts>) {
    let lock = IndexerLock::acquire(dir).unwrap();
    let mut writer = Writer::open(dir).unwrap();
    writer.load(&lock, files, 2, NOW).unwrap();
}

/// `a.rs`: caller -> callee, callee -> unresolved `missing`; `b.rs`: other -> a.rs callee.
fn two_file_graph() -> Vec<graph::FileFacts> {
    let missing = SymbolId::derive(NodeKind::Unresolved, "missing", "");
    vec![
        facts(
            "a.rs",
            vec![
                function("caller", 1),
                function("callee", 10),
                node(NodeKind::Unresolved, "missing", 0),
            ],
            vec![
                call(function_id("a.rs", "caller"), function_id("a.rs", "callee")),
                call(function_id("a.rs", "callee"), missing),
            ],
        ),
        facts(
            "b.rs",
            vec![function("other", 1)],
            vec![call(
                function_id("b.rs", "other"),
                function_id("a.rs", "callee"),
            )],
        ),
    ]
}

#[test]
fn roundtrip_nodes_edges_and_files() {
    let scratch = TestDir::new("roundtrip");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());

    let reader = Reader::open(&dir).unwrap();
    let callee = reader.find("callee", 5).unwrap();
    assert_eq!(callee.len(), 1);
    assert_eq!(callee[0].id, function_id("a.rs", "callee"));
    assert_eq!(callee[0].file.as_deref(), Some("a.rs"));
    assert_eq!(callee[0].line, Some(10));
    assert_eq!(callee[0].degree, 3);

    let callers = reader
        .neighbours(callee[0].id, Direction::In, &[EdgeKind::Calls], 10)
        .unwrap();
    let mut caller_names: Vec<_> = callers
        .items
        .iter()
        .map(|neighbour| neighbour.node.qualified_name.as_str())
        .collect();
    caller_names.sort();
    assert_eq!(caller_names, ["caller", "other"]);
    assert_eq!(callers.total, 2);
    assert!(
        callers
            .items
            .iter()
            .all(|neighbour| neighbour.edge.provenance == Provenance::Extracted)
    );

    let files = reader.files().unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["a.rs", "b.rs"]
    );
    assert_eq!(reader.freshness().unwrap().indexed_at, Some(NOW));
}

#[test]
fn unresolved_calls_are_counted_by_coverage_and_hidden_from_find() {
    let scratch = TestDir::new("coverage");
    let dir = scratch.graph_dir();
    let mut files = two_file_graph();
    let mut secret = file_record(".env", "");
    secret.content_hash = None;
    secret.status = FileStatus::SkippedSecret;
    secret.status_detail = Some("matches secret pattern .env".to_string());
    files.push(graph::FileFacts {
        file: secret,
        nodes: vec![],
        edges: vec![],
    });
    load(&dir, files);

    let reader = Reader::open(&dir).unwrap();
    let coverage = reader.coverage(10).unwrap();
    assert_eq!(coverage.unresolved_edges, 1);
    assert_eq!(coverage.problem_file_total, 1);
    assert_eq!(coverage.problem_files[0].path, ".env");
    assert_eq!(coverage.problem_files[0].status, FileStatus::SkippedSecret);
    let parsed = coverage
        .files
        .iter()
        .find(|count| count.status == FileStatus::Parsed)
        .unwrap();
    assert_eq!(parsed.count, 2);
    assert!(reader.find("missing", 5).unwrap().is_empty());
    assert!(
        reader
            .files()
            .unwrap()
            .iter()
            .find(|file| file.path == ".env")
            .unwrap()
            .content_hash
            .is_none()
    );
}

#[test]
fn symbol_ids_are_stable_across_reindexing() {
    let scratch = TestDir::new("stable-ids");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());
    let before = Reader::open(&dir).unwrap().find("callee", 5).unwrap()[0].id;

    let mut changed = two_file_graph();
    changed[0].nodes[1].line = Some(40);
    changed[0].file = file_record("a.rs", "changed content");
    load(&dir, vec![changed.remove(0)]);

    let reader = Reader::open(&dir).unwrap();
    let after = reader.find("callee", 5).unwrap();
    assert_eq!(after[0].id, before);
    assert_eq!(after[0].line, Some(40));
    // b.rs was not reloaded, yet its edge into a.rs still resolves because ids are hashes.
    let callers = reader.neighbours(before, Direction::In, &[], 10).unwrap();
    assert_eq!(callers.total, 2);
    assert_eq!(function_id("a.rs", "callee"), before);
    assert_ne!(function_id("b.rs", "callee"), before);
}

#[test]
fn id_prefix_and_location_lookups_resolve_directly() {
    let scratch = TestDir::new("direct-lookup");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());
    let reader = Reader::open(&dir).unwrap();
    let id = function_id("a.rs", "callee");

    let by_prefix = reader.find(&format!("@{}", id.short(12)), 5).unwrap();
    assert_eq!(by_prefix.len(), 1);
    assert_eq!(by_prefix[0].id, id);
    assert!(reader.find("@zz", 5).unwrap().is_empty());

    let by_line = reader.find("a.rs:11", 5).unwrap();
    assert_eq!(by_line[0].id, id);
    assert_eq!(reader.find("callee", 5).unwrap()[0].id, id);
    assert_eq!(reader.node(id).unwrap().unwrap().qualified_name, "callee");
}

#[test]
fn fuzzy_find_matches_words_and_prefers_exact_then_first_party() {
    let scratch = TestDir::new("fuzzy");
    let dir = scratch.graph_dir();
    let mut vendored = facts("vendor/lib.rs", vec![function("getUserName", 1)], vec![]);
    vendored.file.tags.vendored = true;
    load(
        &dir,
        vec![
            vendored,
            facts(
                "user.rs",
                vec![
                    function("get_user_name", 1),
                    function("get_user_name_cached", 5),
                ],
                vec![],
            ),
        ],
    );
    let reader = Reader::open(&dir).unwrap();

    let found = reader.find("GetUserName", 10).unwrap();
    let paths: Vec<_> = found
        .iter()
        .map(|node| (node.file.as_deref().unwrap(), node.name.as_str()))
        .collect();
    assert_eq!(paths.len(), 3);
    assert_eq!(paths.last().unwrap().0, "vendor/lib.rs");
    assert_eq!(found[0].name, "get_user_name");
    assert_eq!(
        reader.find("user cach", 10).unwrap()[0].name,
        "get_user_name_cached"
    );
}

#[test]
fn reloading_a_file_replaces_its_nodes_and_edges() {
    let scratch = TestDir::new("replace");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());

    let shrunk = facts(
        "a.rs",
        vec![function("caller", 1), function("callee", 10)],
        vec![call(
            function_id("a.rs", "caller"),
            function_id("a.rs", "callee"),
        )],
    );
    load(&dir, vec![shrunk]);

    let reader = Reader::open(&dir).unwrap();
    let coverage = reader.coverage(0).unwrap();
    assert_eq!(coverage.unresolved_edges, 0);
    let callee = reader.find("callee", 1).unwrap()[0].clone();
    assert_eq!(callee.degree, 2);
    let missing = SymbolId::derive(NodeKind::Unresolved, "missing", "");
    assert!(
        reader.node(missing).unwrap().is_none(),
        "orphaned unresolved node is dropped"
    );
}

#[test]
fn removing_a_file_drops_everything_it_contributed() {
    let scratch = TestDir::new("remove");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());

    let lock = IndexerLock::acquire(&dir).unwrap();
    let mut writer = Writer::open(&dir).unwrap();
    let removed = writer
        .remove_files(&lock, &["b.rs".to_string(), "gone.rs".to_string()], NOW)
        .unwrap();
    assert_eq!(removed, 1);

    let reader = Reader::open(&dir).unwrap();
    assert!(reader.find("other", 5).unwrap().is_empty());
    assert_eq!(reader.files().unwrap().len(), 1);
    assert_eq!(reader.find("callee", 1).unwrap()[0].degree, 2);
}

#[test]
fn loader_rejects_bad_facts_loudly() {
    let scratch = TestDir::new("invalid");
    let dir = scratch.graph_dir();
    let lock = IndexerLock::acquire(&dir).unwrap();
    let mut writer = Writer::open(&dir).unwrap();

    let mut inferred = edge(
        function_id("a.rs", "x"),
        function_id("a.rs", "y"),
        EdgeKind::Calls,
    );
    inferred.provenance = Provenance::Inferred;
    let without_reason = facts("a.rs", vec![function("x", 1)], vec![inferred.clone()]);
    assert!(matches!(
        writer.load(&lock, [without_reason], 10, NOW),
        Err(Error::InvalidFact(message)) if message.contains("no reason")
    ));
    inferred.reason = Some("same name in enclosing class".to_string());
    let with_reason = facts("a.rs", vec![function("x", 1)], vec![inferred]);
    writer.load(&lock, [with_reason], 10, NOW).unwrap();

    let duplicate = facts("d.rs", vec![function("dup", 1), function("dup", 9)], vec![]);
    assert!(matches!(
        writer.load(&lock, [duplicate], 10, NOW),
        Err(Error::InvalidFact(message)) if message.contains("duplicate symbol")
    ));
}

#[test]
fn reader_needs_an_existing_graph() {
    let scratch = TestDir::new("not-indexed");
    assert!(matches!(
        Reader::open(&scratch.graph_dir()),
        Err(Error::NotIndexed(_))
    ));
}

#[test]
fn components_descriptions_and_staleness() {
    let scratch = TestDir::new("components");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());
    let hash = |text: &str| ContentHash::of_bytes(text.as_bytes());
    let component = |id, parent: Option<i64>, level, name: &str| Component {
        id: ComponentId(id),
        parent: parent.map(ComponentId),
        level,
        name: name.to_string(),
        member_hash: hash(name),
        stale: false,
    };

    let mut writer = Writer::open(&dir).unwrap();
    writer
        .replace_components(
            &[
                component(1, None, 0, "system"),
                component(2, Some(1), 1, "core"),
            ],
            &[
                ("a.rs".to_string(), ComponentId(2)),
                ("b.rs".to_string(), ComponentId(2)),
            ],
        )
        .unwrap();
    let description = Description {
        component: ComponentId(2),
        component_hash: hash("core"),
        name: "Core".to_string(),
        summary: "Does the work.".to_string(),
        tags: vec!["engine".to_string(), "hot".to_string()],
        written_at: NOW,
    };
    writer.put_description(&description).unwrap();

    let reader = Reader::open(&dir).unwrap();
    let top = Query::components(&reader, None).unwrap();
    assert_eq!(
        top.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["system"]
    );
    let children = Query::components(&reader, Some(ComponentId(1))).unwrap();
    assert_eq!(children[0].id, ComponentId(2));
    assert_eq!(
        reader.description(ComponentId(2)).unwrap(),
        Some(description.clone())
    );
    assert!(!description.is_stale_for(&children[0]));
    let symbols = reader.top_symbols(ComponentId(1), 2).unwrap();
    assert_eq!(symbols[0].name, "callee");
    assert_eq!(reader.files().unwrap()[0].component, Some(ComponentId(2)));

    let lock = IndexerLock::acquire(&dir).unwrap();
    writer
        .load(&lock, vec![chain_file("b.rs", "n", 2)], 10, NOW)
        .unwrap();
    let reader = Reader::open(&dir).unwrap();
    assert!(reader.component(ComponentId(2)).unwrap().unwrap().stale);
    assert!(
        reader.component(ComponentId(1)).unwrap().unwrap().stale,
        "ancestors go stale too"
    );
}

#[test]
fn file_links_count_cross_file_edges() {
    let scratch = TestDir::new("file-links");
    let dir = scratch.graph_dir();
    load(&dir, two_file_graph());
    let reader = Reader::open(&dir).unwrap();
    let files = reader.files().unwrap();
    let links = reader.file_links().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(
        (links[0].from_file, links[0].to_file, links[0].edge_count),
        (files[1].id, files[0].id, 1)
    );
}

#[test]
fn global_node_ids_ignore_the_file() {
    let first = SymbolId::derive(NodeKind::Hook, "init", "a.php");
    let second = SymbolId::derive(NodeKind::Hook, "init", "b.php");
    assert_eq!(first, second);
    assert_ne!(
        SymbolId::derive(NodeKind::Function, "init", "a.php"),
        SymbolId::derive(NodeKind::Function, "init", "b.php")
    );
    assert!(first.as_i64() >= 0);
}

/// Everything derived from the rows: degrees, the word index and the secondary indexes.
#[derive(Debug, PartialEq, Eq)]
struct DerivedData {
    degrees: Vec<(i64, i64)>,
    words: Vec<(i64, String)>,
    indexes: Vec<String>,
}

fn derived_data(dir: &graph::GraphDir) -> DerivedData {
    let connection = rusqlite::Connection::open(dir.database_path()).unwrap();
    let degrees = connection
        .prepare("SELECT id, degree FROM nodes ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let words = connection
        .prepare("SELECT rowid, words FROM node_words ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut indexes: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    indexes.sort();
    DerivedData {
        degrees,
        words,
        indexes,
    }
}

#[test]
fn a_first_load_builds_the_same_derived_data_as_loading_file_by_file() {
    let at_once = TestDir::new("load-bulk");
    load(&at_once.graph_dir(), two_file_graph());

    let one_by_one = TestDir::new("load-incremental");
    let mut files = two_file_graph();
    let second = files.pop().unwrap();
    load(&one_by_one.graph_dir(), files);
    load(&one_by_one.graph_dir(), vec![second]);

    let bulk = derived_data(&at_once.graph_dir());
    assert_eq!(bulk, derived_data(&one_by_one.graph_dir()));
    assert!(
        bulk.indexes.contains(&"edges_out".to_string())
            && bulk.indexes.contains(&"nodes_file".to_string()),
        "a bulk load must leave every secondary index built: {:?}",
        bulk.indexes
    );
}

#[test]
fn a_first_load_that_fails_leaves_no_graph_behind() {
    let dir = TestDir::new("load-bulk-fails");
    let mut files = two_file_graph();
    files[1].edges[0].provenance = Provenance::Inferred;
    let lock = IndexerLock::acquire(&dir.graph_dir()).unwrap();
    let mut writer = Writer::open(&dir.graph_dir()).unwrap();
    assert!(matches!(
        writer.load(&lock, files, 2, NOW),
        Err(Error::InvalidFact(_))
    ));
    drop(writer);
    let left_behind = derived_data(&dir.graph_dir());
    assert!(left_behind.degrees.is_empty() && left_behind.words.is_empty());
    assert!(
        left_behind.indexes.contains(&"edges_out".to_string()),
        "the rollback restores the indexes"
    );
}
