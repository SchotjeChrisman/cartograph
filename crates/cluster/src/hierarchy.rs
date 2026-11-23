//! The file graph and the nested communities found on it, one Leiden pass per level.

use std::collections::HashMap;

use graph::{FileLink, FileRow};

use crate::leiden;
use crate::weighted::WeightedGraph;

/// A community of files and the finer communities inside it. Leaves of the whole tree have no
/// children; the files themselves are the level below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    /// Indexes into the file slice the hierarchy was built from, ascending.
    pub members: Vec<usize>,
    pub children: Vec<Cluster>,
}

/// Files as nodes and cross-file edges as weights. Files in one directory are also chained to
/// each other with `directory_weight`, so unrelated-looking siblings still gravitate together
/// and the result matches the layout developers expect.
pub fn file_graph(files: &[&FileRow], links: &[FileLink], directory_weight: f64) -> WeightedGraph {
    let mut graph = WeightedGraph::with_nodes(files.len());
    let index_of: HashMap<i64, usize> = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.id, index))
        .collect();
    for link in links {
        if let (Some(from), Some(to)) = (index_of.get(&link.from_file), index_of.get(&link.to_file))
        {
            graph.add_edge(*from, *to, link.edge_count as f64);
        }
    }
    for siblings in directory_groups(files).values() {
        for pair in siblings.windows(2) {
            graph.add_edge(pair[0], pair[1], directory_weight);
        }
        if siblings.len() > 2 {
            graph.add_edge(siblings[siblings.len() - 1], siblings[0], directory_weight);
        }
    }
    graph
}

/// The directory part of a path, empty for a file at the root.
pub fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn directory_groups<'a>(files: &[&'a FileRow]) -> HashMap<&'a str, Vec<usize>> {
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        groups
            .entry(directory_of(&file.path))
            .or_default()
            .push(index);
    }
    groups
}

/// Nested clusters, one level per entry of `resolutions` (coarsest first). Each level splits the
/// clusters of the one above, so the result is a strict tree.
pub fn build(files: &[&FileRow], graph: &WeightedGraph, resolutions: &[f64]) -> Vec<Cluster> {
    let everything: Vec<usize> = (0..files.len()).collect();
    split(
        files,
        graph,
        graph.doubled_total(),
        &everything,
        resolutions,
    )
}

fn split(
    files: &[&FileRow],
    graph: &WeightedGraph,
    doubled_total: f64,
    members: &[usize],
    resolutions: &[f64],
) -> Vec<Cluster> {
    let Some((resolution, finer)) = resolutions.split_first() else {
        return Vec::new();
    };
    let subgraph = graph.induced(members);
    let seed = directory_seed(files, members);
    let labels = leiden::partition(&subgraph, &seed, *resolution, doubled_total);
    let group_count = labels.iter().max().map_or(0, |max| max + 1);
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); group_count];
    for (local, label) in labels.iter().enumerate() {
        groups[*label].push(members[local]);
    }
    groups
        .into_iter()
        .map(|group| Cluster {
            children: split(files, graph, doubled_total, &group, finer),
            members: group,
        })
        .collect()
}

/// Starting labels that put every directory in its own community.
fn directory_seed(files: &[&FileRow], members: &[usize]) -> Vec<usize> {
    let mut label_of: HashMap<&str, usize> = HashMap::new();
    members
        .iter()
        .map(|member| {
            let next = label_of.len();
            *label_of
                .entry(directory_of(&files[*member].path))
                .or_insert(next)
        })
        .collect()
}

/// Replaces every cluster that has a single child covering the same files by that child, so a
/// small project does not show three identical boxes in a row.
pub fn collapse(clusters: Vec<Cluster>) -> Vec<Cluster> {
    clusters
        .into_iter()
        .map(|cluster| {
            let mut children = collapse(cluster.children);
            if children.len() == 1 {
                return children.remove(0);
            }
            Cluster {
                children,
                ..cluster
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_of_handles_root_and_nested_files() {
        assert_eq!(directory_of("main.rs"), "");
        assert_eq!(directory_of("src/a/b.rs"), "src/a");
    }

    #[test]
    fn collapse_removes_single_child_chains_only() {
        let leaf = |members: Vec<usize>| Cluster {
            members,
            children: vec![],
        };
        let chain = Cluster {
            members: vec![0, 1],
            children: vec![Cluster {
                members: vec![0, 1],
                children: vec![leaf(vec![0, 1])],
            }],
        };
        let branching = Cluster {
            members: vec![2, 3],
            children: vec![leaf(vec![2]), leaf(vec![3])],
        };

        let collapsed = collapse(vec![chain, branching.clone()]);

        assert_eq!(collapsed[0], leaf(vec![0, 1]));
        assert_eq!(collapsed[1], branching);
    }
}
