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

/// The directory a query reads: the explicit root, else the nearest ancestor of the working
/// directory that holds a graph, else the one linked worktree inside it that holds a graph, else
/// the working directory itself (which then reports "no graph" with the path it looked at).
pub fn graph_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if explicit.is_some() {
        return index_root(explicit);
    }
    let start = std::env::current_dir()?.canonicalize()?;
    let found = start
        .ancestors()
        .find(|candidate| GraphDir::of_root(candidate).database_path().exists());
    if let Some(found) = found {
        return Ok(found.to_path_buf());
    }
    Ok(only_worktree_graph(&start)?.unwrap_or(start))
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
