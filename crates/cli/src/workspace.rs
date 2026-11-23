//! Which directory a command works on.

use std::path::{Path, PathBuf};

use graph::GraphDir;

use crate::Result;

/// The directory `index` indexes: the explicit root, else the working directory.
pub fn index_root(explicit: Option<&Path>) -> Result<PathBuf> {
    Ok(match explicit {
        Some(root) => root.canonicalize()?,
        None => std::env::current_dir()?.canonicalize()?,
    })
}

/// The directory a query reads: the explicit root, else the nearest ancestor of the working
/// directory that holds a graph, else the working directory itself (which then reports "no
/// graph" with the path it looked at).
pub fn graph_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if explicit.is_some() {
        return index_root(explicit);
    }
    let start = std::env::current_dir()?.canonicalize()?;
    let found = start
        .ancestors()
        .find(|candidate| GraphDir::of_root(candidate).database_path().exists());
    Ok(found.unwrap_or(&start).to_path_buf())
}
