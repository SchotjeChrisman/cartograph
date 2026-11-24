//! A bare-clone project: `.bare` holds the repository and each checkout is a linked worktree
//! beside it. From the project's top, commands use the one worktree that holds a graph and never
//! write a graph of their own there.

use crate::common::{TestDir, ok, run};

const SOURCE: &str = "pub fn leaf() {}\n";

fn bare_clone(name: &str, worktrees: &[&str]) -> TestDir {
    let project = TestDir::new(name);
    lay_out_bare_clone(&project, "", worktrees);
    project
}

/// Writes the layout into `top`, under `prefix` (empty, or a folder name ending in `/`).
fn lay_out_bare_clone(top: &TestDir, prefix: &str, worktrees: &[&str]) {
    top.write(&format!("{prefix}.git"), "gitdir: ./.bare\n");
    top.write(&format!("{prefix}.bare/config"), "[core]\n\tbare = true\n");
    top.write(&format!("{prefix}.shared/stray.rs"), SOURCE);
    for worktree in worktrees {
        top.write(&format!("{prefix}{worktree}/src/lib.rs"), SOURCE);
        top.write(
            &format!("{prefix}{worktree}/.git"),
            &format!("gitdir: ../.bare/worktrees/{worktree}\n"),
        );
    }
}

#[test]
fn the_worktree_graph_wins_over_a_graph_further_up() {
    let outer = TestDir::new("bare-outer");
    outer.write("elsewhere.rs", "pub fn elsewhere() {}\n");
    ok(outer.path(), &["index"]);
    lay_out_bare_clone(&outer, "project/", &["main", "develop"]);
    let project = outer.path().join("project");
    ok(&project.join("develop"), &["index"]);

    let found = ok(&project, &["find", "leaf"]);

    assert!(found.contains("src/lib.rs"), "{found}");
    assert!(!project.join(".cartograph").exists());
}

#[test]
fn the_top_uses_the_one_worktree_with_a_graph() {
    let project = bare_clone("bare-one", &["main", "develop"]);
    ok(&project.path().join("develop"), &["index"]);

    let found = ok(project.path(), &["find", "leaf"]);
    assert!(found.contains("src/lib.rs"), "{found}");
    let indexed = ok(project.path(), &["index"]);
    assert!(indexed.starts_with("indexed 1 files"), "{indexed}");

    assert!(!project.path().join(".cartograph").exists());
    assert!(!project.path().join("main/.cartograph").exists());
}

#[test]
fn two_worktrees_with_a_graph_ask_for_root() {
    let project = bare_clone("bare-two", &["main", "develop"]);
    ok(&project.path().join("develop"), &["index"]);
    ok(&project.path().join("main"), &["index"]);

    for args in [&["find", "leaf"][..], &["index"]] {
        let output = run(project.path(), args);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(
            error.contains("develop") && error.contains("main") && error.contains("--root"),
            "{error}"
        );
    }
    assert!(!project.path().join(".cartograph").exists());
}

#[test]
fn indexing_the_top_without_a_worktree_graph_writes_nothing() {
    let project = bare_clone("bare-none", &["main", "develop"]);

    let output = run(project.path(), &["index"]);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("develop, main"), "{error}");
    assert!(!project.path().join(".cartograph").exists());
}
