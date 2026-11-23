use crate::common::{add_function, indexed_project, ok, rust_project};

const REINDEX_NOTE: &str = "re-indexed";

#[test]
fn a_few_changed_files_are_re_indexed_before_answering_and_only_once() {
    let project = indexed_project("stale-few");
    let dir = project.path();
    assert!(!ok(dir, &["find", "leaf"]).contains(REINDEX_NOTE));

    add_function(
        &project,
        "crates/core_lib/src/util.rs",
        "brand_new_function",
    );
    let first = ok(dir, &["find", "brand_new_function"]);
    assert!(first.contains(REINDEX_NOTE), "{first}");
    assert!(first.contains("util.rs"), "{first}");

    let second = ok(dir, &["find", "brand_new_function"]);
    assert!(!second.contains(REINDEX_NOTE), "{second}");
    assert!(second.contains("util.rs"), "{second}");
}

#[test]
fn a_deleted_file_is_dropped_from_the_graph() {
    let project = indexed_project("stale-delete");
    let dir = project.path();
    assert!(ok(dir, &["find", "stop"]).contains("local.rs"));
    std::fs::remove_file(dir.join("crates/app/src/local.rs")).unwrap();
    let after = ok(dir, &["find", "stop"]);
    assert!(after.contains(REINDEX_NOTE), "{after}");
    assert!(!after.contains("local.rs"), "{after}");
}

#[test]
fn many_changed_files_only_warn() {
    let project = rust_project("stale-many");
    let dir = project.path();
    ok(dir, &["index"]);
    for number in 0..120 {
        project.write(
            &format!("bulk/file_{number}.rs"),
            &format!("pub fn bulk_{number}() {{}}\n"),
        );
    }
    let answer = ok(dir, &["find", "bulk_7"]);
    assert!(answer.contains("120 files changed"), "{answer}");
    assert!(!answer.contains(REINDEX_NOTE), "{answer}");
    assert!(!answer.contains("bulk/file_7.rs"), "{answer}");

    ok(dir, &["index"]);
    assert!(ok(dir, &["find", "bulk_7"]).contains("bulk/file_7.rs"));
}
