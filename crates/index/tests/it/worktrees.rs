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
