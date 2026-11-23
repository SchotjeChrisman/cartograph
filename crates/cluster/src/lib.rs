//! Hierarchical Leiden clustering of the file graph into system > subsystem > component > file.
//!
//! [`compute`] is pure: files and links in, tree out. [`recluster`] and [`mark_stale`] are the
//! thin edges that read the graph file and store the result.

mod hierarchy;
mod identity;
mod leiden;
mod staleness;
mod tree;
mod weighted;

use graph::{FileLink, FileRow, FileStatus, Reader, Writer};

pub use identity::Previous;
pub use staleness::{Staleness, check_staleness};
pub use tree::Tree;

/// Resolutions for system, subsystem and component, coarsest first.
pub const DEFAULT_RESOLUTIONS: [f64; 3] = [0.3, 1.0, 3.0];

/// How strongly files in one directory are pulled together, relative to one call between files.
pub const DEFAULT_DIRECTORY_WEIGHT: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct Config {
    /// One Leiden resolution per level below the root, coarsest first.
    pub resolutions: Vec<f64>,
    pub directory_weight: f64,
    /// Vendored and generated files are left out of components unless this is set.
    pub include_vendored: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            resolutions: DEFAULT_RESOLUTIONS.to_vec(),
            directory_weight: DEFAULT_DIRECTORY_WEIGHT,
            include_vendored: false,
        }
    }
}

/// Whether a file takes part in components. Unparsed files have no edges to cluster on, and
/// secrets and nested graphs must stay out entirely.
pub(crate) fn is_clustered(file: &FileRow, config: &Config) -> bool {
    let excluded_by_tag = file.tags.vendored || file.tags.generated;
    file.status == FileStatus::Parsed && (config.include_vendored || !excluded_by_tag)
}

/// Builds the component tree for `files` (in path order), reusing ids from `previous`.
pub fn compute(
    files: &[FileRow],
    links: &[FileLink],
    previous: &Previous,
    config: &Config,
) -> Tree {
    let clustered: Vec<&FileRow> = files.iter().filter(|f| is_clustered(f, config)).collect();
    let file_graph = hierarchy::file_graph(&clustered, links, config.directory_weight);
    let clusters = hierarchy::collapse(hierarchy::build(
        &clustered,
        &file_graph,
        &config.resolutions,
    ));
    tree::build(&clustered, &clusters, previous)
}

/// Reads the graph, computes the tree and stores it, replacing the old one.
pub fn recluster(reader: &Reader, writer: &mut Writer, config: &Config) -> graph::Result<Tree> {
    let previous = Previous::load(reader)?;
    let tree = compute(&reader.files()?, &reader.file_links()?, &previous, config);
    writer.replace_components(&tree.components, &tree.file_components)?;
    Ok(tree)
}

/// Flags the stored components whose files changed since they were computed. Returns what it
/// found; a nonzero `unassigned_files` means a recluster is due, not just stale flags.
pub fn mark_stale(
    reader: &Reader,
    writer: &mut Writer,
    config: &Config,
) -> graph::Result<Staleness> {
    let previous = Previous::load(reader)?;
    let staleness = check_staleness(&reader.files()?, &previous, config);
    let mut components = previous.components.clone();
    for component in &mut components {
        component.stale |= staleness.stale.contains(&component.id);
    }
    writer.replace_components(&components, &previous.file_components)?;
    Ok(staleness)
}
