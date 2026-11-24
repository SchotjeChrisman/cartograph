//! Git worktrees below the root are other checkouts of the same code: indexing them would
//! duplicate every definition, so the walk skips them. Submodules are different code and stay.

use crate::common::{TestDir, dump, index};

const SOURCE: &str = "pub fn f() {}\n";

fn project_with_worktree_and_submodule() -> TestDir {
    let tree = TestDir::new("worktrees");
    tree.write("src/lib.rs", SOURCE);
    tree.write("wt/src/lib.rs", SOURCE);
    tree.write("wt/.git", "gitdir: /home/x/project/.bare/worktrees/wt\n");
    tree.write("sub/src/lib.rs", SOURCE);
    tree.write("sub/.git", "gitdir: ../.git/modules/sub\n");
    tree
}

#[test]
fn a_linked_worktree_is_skipped_and_a_submodule_is_kept() {
    let tree = project_with_worktree_and_submodule();
    index(tree.path());

    let files = dump(tree.path(), false);

    assert!(!files.contains("wt/src/lib.rs"), "{files}");
    assert!(files.contains("sub/src/lib.rs"), "{files}");
    assert!(files.contains("src/lib.rs"), "{files}");
}

#[test]
fn a_root_holding_only_worktrees_is_not_indexed() {
    let tree = TestDir::new("worktrees-only");
    for name in ["main", "feature"] {
        tree.write(&format!("{name}/src/lib.rs"), SOURCE);
        tree.write(
            &format!("{name}/.git"),
            &format!("gitdir: /p/.bare/worktrees/{name}\n"),
        );
    }

    let error = index::index_directory(tree.path(), &index::IndexOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("feature, main"), "{error}");
    assert!(!tree.path().join(".cartograph").exists());
}

/// `<project>/.bare` holds the repository, `<project>/.git` points at it, and every checkout is
/// a linked worktree beside them; `.shared` stands for files that belong to no checkout.
fn bare_clone(name: &str, worktrees: &[&str]) -> TestDir {
    let tree = TestDir::new(name);
    tree.write(".git", "gitdir: ./.bare\n");
    tree.write(".bare/config", "[core]\n\tbare = true\n");
    tree.write(".shared/notes.rs", SOURCE);
    for worktree in worktrees {
        tree.write(&format!("{worktree}/src/lib.rs"), SOURCE);
        tree.write(
            &format!("{worktree}/.git"),
            &format!("gitdir: ../.bare/worktrees/{worktree}\n"),
        );
    }
    tree
}

#[test]
fn a_bare_clone_root_is_not_indexed_despite_stray_files() {
    let tree = bare_clone("bare-root", &["main", "develop"]);

    let error = index::index_directory(tree.path(), &index::IndexOptions::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("develop, main"), "{error}");
    assert!(!tree.path().join(".cartograph").exists());
    assert!(index::is_bare_clone_root(tree.path()));
    assert!(!index::is_bare_clone_root(&tree.path().join("main")));
}

#[test]
fn worktrees_with_graph_lists_only_indexed_worktrees() {
    let tree = bare_clone("bare-graphs", &["main", "develop", "feature"]);
    assert!(index::worktrees_with_graph(tree.path()).is_empty());

    index(&tree.path().join("develop"));
    assert_eq!(
        index::worktrees_with_graph(tree.path()),
        [tree.path().join("develop")]
    );

    index(&tree.path().join("main"));
    assert_eq!(
        index::worktrees_with_graph(tree.path()),
        [tree.path().join("develop"), tree.path().join("main")]
    );
}
