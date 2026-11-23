//! What the walk skips, tags and reports, and what it must never store.

use crate::common::{copy_fixture, index};
use graph::{GraphDir, IndexerLock};
use index::{Error, IndexOptions, index_directory};

#[test]
fn nested_graphs_are_reported_and_secrets_never_reach_the_database() {
    let tree = copy_fixture("cpp", "coverage-cpp");
    let report = index(tree.path());
    assert_eq!(report.nested_graphs, ["sub/.cartograph"]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let database = std::fs::read(tree.path().join(".cartograph/graph.db")).unwrap();
    let wal = std::fs::read(tree.path().join(".cartograph/graph.db-wal")).unwrap_or_default();
    for secret in ["TOKEN=abc", "PRIVATE KEY"] {
        let needle = secret.as_bytes();
        assert!(
            !database
                .windows(needle.len())
                .any(|window| window == needle),
            "{secret} leaked into the db"
        );
        assert!(
            !wal.windows(needle.len()).any(|window| window == needle),
            "{secret} leaked into the wal"
        );
    }
}

#[test]
fn a_second_indexer_fails_at_once_while_the_lock_is_held() {
    let tree = copy_fixture("rust", "coverage-lock");
    let _held = IndexerLock::acquire(&GraphDir::of_root(tree.path())).unwrap();
    let result = index_directory(tree.path(), &IndexOptions::default());
    assert!(
        matches!(result, Err(Error::Graph(graph::Error::IndexerLockHeld))),
        "{result:?}"
    );
}

#[test]
fn a_broken_config_is_an_error_not_a_silent_default() {
    let tree = copy_fixture("rust", "coverage-config");
    tree.write(".cartograph/config.toml", "vendored = 3\n");
    let result = index_directory(tree.path(), &IndexOptions::default());
    assert!(matches!(result, Err(Error::Config { .. })), "{result:?}");
}

#[test]
fn ambiguous_names_stay_unresolved_nodes_in_every_new_language() {
    for (fixture, unresolved) in [
        ("script", "unresolved render"),
        ("php", "unresolved format"),
        (
            "laravel",
            "unresolved App\\Http\\Controllers\\UserController::audit",
        ),
        ("links", "unresolved send"),
    ] {
        let tree = copy_fixture(fixture, &format!("coverage-ambiguous-{fixture}"));
        index(tree.path());
        let dump = crate::common::dump(tree.path(), false);
        assert!(
            dump.contains(unresolved),
            "{fixture} must keep `{unresolved}` visible:\n{dump}"
        );
    }
}

#[test]
fn a_malformed_links_file_is_reported_not_skipped() {
    let tree = crate::common::TestDir::new("coverage-links-malformed");
    tree.write("links.yaml", "links:\n  - to: [broken]\n    kind: x\n");
    index(tree.path());
    let dump = crate::common::dump(tree.path(), false);
    assert!(
        dump.contains("links.yaml repo= lang=links status=parse_failed"),
        "{dump}"
    );
    assert!(dump.contains("put `[broken]` in quotes"), "{dump}");
}
