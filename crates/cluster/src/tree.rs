//! Turns clusters of files into stored components: ids, names and member hashes.

use std::collections::HashMap;

use graph::{Component, ComponentId, ContentHash, FileRow};

use crate::hierarchy::{Cluster, directory_of};
use crate::identity::{Candidate, Previous, assign_ids};

/// The component tree and which component each clustered file sits in; exactly what
/// [`graph::Writer::replace_components`] stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub components: Vec<Component>,
    pub file_components: Vec<(String, ComponentId)>,
}

/// A cluster flattened to a row, parents before children.
struct Draft<'a> {
    parent: Option<usize>,
    level: u32,
    members: &'a [usize],
    is_leaf: bool,
}

pub fn build(files: &[&FileRow], clusters: &[Cluster], previous: &Previous) -> Tree {
    let mut drafts = Vec::new();
    flatten(clusters, None, 0, &mut drafts);

    let candidates: Vec<Candidate> = drafts
        .iter()
        .map(|draft| Candidate {
            level: draft.level,
            paths: draft
                .members
                .iter()
                .map(|m| files[*m].path.as_str())
                .collect(),
        })
        .collect();
    let ids = assign_ids(&candidates, previous);
    let names = sibling_names(files, &drafts);

    let components = drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| Component {
            id: ids[index],
            parent: draft.parent.map(|parent| ids[parent]),
            level: draft.level,
            name: names[index].clone(),
            member_hash: member_hash(draft.members.iter().map(|m| files[*m])),
            stale: false,
        })
        .collect();
    let file_components = drafts
        .iter()
        .enumerate()
        .filter(|(_, draft)| draft.is_leaf)
        .flat_map(|(index, draft)| {
            let id = ids[index];
            draft
                .members
                .iter()
                .map(move |m| (files[*m].path.clone(), id))
        })
        .collect();
    Tree {
        components,
        file_components,
    }
}

fn flatten<'a>(
    clusters: &'a [Cluster],
    parent: Option<usize>,
    level: u32,
    drafts: &mut Vec<Draft<'a>>,
) {
    for cluster in clusters {
        let index = drafts.len();
        drafts.push(Draft {
            parent,
            level,
            members: &cluster.members,
            is_leaf: cluster.children.is_empty(),
        });
        flatten(&cluster.children, Some(index), level + 1, drafts);
    }
}

/// Hash of a component's files and their contents, so any change to a member changes it.
/// Files must come in path order.
pub fn member_hash<'a>(files: impl Iterator<Item = &'a FileRow>) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update(&[0]);
        if let Some(hash) = &file.content_hash {
            hasher.update(hash.as_bytes());
        }
    }
    ContentHash::from_slice(hasher.finalize().as_bytes()).expect("blake3 gives 32 bytes")
}

/// Names that match what developers see in the tree, made unique among siblings.
fn sibling_names(files: &[&FileRow], drafts: &[Draft]) -> Vec<String> {
    let mut uses: HashMap<(Option<usize>, String), usize> = HashMap::new();
    drafts
        .iter()
        .map(|draft| {
            let name = label(draft.members.iter().map(|m| files[*m].path.as_str()));
            let count = uses.entry((draft.parent, name.clone())).or_default();
            *count += 1;
            if *count == 1 {
                name
            } else {
                format!("{name} #{count}")
            }
        })
        .collect()
}

/// The directory all paths share; a lone file is named by its path, and files with nothing in
/// common by the most frequent top-level directory.
fn label<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let paths: Vec<&str> = paths.collect();
    if let [only] = paths.as_slice() {
        return (*only).to_string();
    }
    let common = common_directory(&paths);
    if !common.is_empty() {
        return common;
    }
    let mut tops: HashMap<&str, usize> = HashMap::new();
    for path in &paths {
        let top = path
            .split('/')
            .next()
            .filter(|_| path.contains('/'))
            .unwrap_or("(root)");
        *tops.entry(top).or_default() += 1;
    }
    tops.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
        .map_or_else(|| "(root)".to_string(), |(top, _)| top.to_string())
}

fn common_directory(paths: &[&str]) -> String {
    let mut common: Vec<&str> = match paths.first() {
        Some(first) => directory_of(first)
            .split('/')
            .filter(|s| !s.is_empty())
            .collect(),
        None => return String::new(),
    };
    for path in &paths[1..] {
        let directory: Vec<&str> = directory_of(path)
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let shared = common
            .iter()
            .zip(&directory)
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(shared);
    }
    common.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_uses_the_shared_directory() {
        assert_eq!(label(["a/b/x.rs", "a/b/c/y.rs"].into_iter()), "a/b");
    }

    #[test]
    fn label_falls_back_to_the_most_common_top_directory() {
        assert_eq!(label(["a/x.rs", "a/y.rs", "b/z.rs"].into_iter()), "a");
    }

    #[test]
    fn a_lone_file_is_named_by_its_path() {
        assert_eq!(label(["a/x.rs"].into_iter()), "a/x.rs");
    }

    #[test]
    fn root_files_are_named_root() {
        assert_eq!(label(["x.rs", "y.rs"].into_iter()), "(root)");
    }
}
