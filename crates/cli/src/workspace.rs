//! Which directory a command works on.

use std::path::{Path, PathBuf};

use graph::GraphDir;

use crate::Result;

/// The directory `install` works on: the explicit root, else the working directory.
pub fn index_root(explicit: Option<&Path>) -> Result<PathBuf> {
    Ok(match explicit {
        Some(root) => root.canonicalize()?,
        None => std::env::current_dir()?.canonicalize()?,
    })
}

/// The directory `index` indexes: the explicit root, else the working directory, except at the
/// top of a bare-clone layout, where it is the one linked worktree that already holds a graph.
/// Without one the working directory stays, and the index refuses it.
pub fn index_target(explicit: Option<&Path>) -> Result<PathBuf> {
    let root = index_root(explicit)?;
    if explicit.is_some() || !index::is_bare_clone_root(&root) {
        return Ok(root);
    }
    Ok(only_worktree_graph(&root)?.unwrap_or(root))
}

/// The directory a query reads: the explicit root, else the working directory when it holds a
/// graph, else the one linked worktree inside it that holds a graph, else the nearest ancestor
/// that holds one, else the working directory itself (which then reports "no graph" with the path
/// it looked at). The worktree comes before the ancestors so a stray graph higher up, such as one
/// in the home folder, cannot hide the project's own.
pub fn graph_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if explicit.is_some() {
        return index_root(explicit);
    }
    let start = std::env::current_dir()?.canonicalize()?;
    if !holds_graph(&start)
        && let Some(worktree) = only_worktree_graph(&start)?
    {
        return Ok(worktree);
    }
    let found = start.ancestors().find(|candidate| holds_graph(candidate));
    Ok(found.unwrap_or(&start).to_path_buf())
}

fn holds_graph(directory: &Path) -> bool {
    GraphDir::of_root(directory).database_path().exists()
}

/// The single linked worktree directly inside `directory` that holds a graph; an error when
/// several do, since picking one would be a guess.
fn only_worktree_graph(directory: &Path) -> Result<Option<PathBuf>> {
    let mut found = index::worktrees_with_graph(directory);
    if found.len() > 1 {
        let names: Vec<String> = found
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        return Err(format!(
            "several worktrees in {} hold a graph: {}; pass --root to pick one",
            directory.display(),
            names.join(", ")
        )
        .into());
    }
    Ok(found.pop())
}
