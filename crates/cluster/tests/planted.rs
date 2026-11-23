//! Clustering on graphs with known community structure.

use std::collections::{HashMap, HashSet};

use cluster::{Config, Previous, check_staleness, compute};
use graph::{ComponentId, ContentHash, FileLink, FileRow, FileStatus, FileTags};

fn file(id: i64, path: &str) -> FileRow {
    FileRow {
        id,
        path: path.to_string(),
        repo: String::new(),
        language: Some("rust".to_string()),
        content_hash: Some(ContentHash::of_bytes(path.as_bytes())),
        size_bytes: 0,
        mtime_ns: None,
        status: FileStatus::Parsed,
        tags: FileTags::default(),
        component: None,
    }
}

fn link(from: i64, to: i64, edge_count: u64) -> FileLink {
    FileLink {
        from_file: from,
        to_file: to,
        edge_count,
    }
}

/// Four communities of eight files whose members are spread over four directories, so a
/// directory-only grouping would get every one of them wrong.
fn planted() -> (Vec<FileRow>, Vec<FileLink>, Vec<Vec<String>>) {
    let mut files = Vec::new();
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); 4];
    for group in 0..4i64 {
        for member in 0..8i64 {
            let id = group * 8 + member + 1;
            let path = format!("d{}/g{group}_f{member}.rs", member % 4);
            groups[group as usize].push(path.clone());
            files.push(file(id, &path));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut links = Vec::new();
    for group in 0..4i64 {
        for a in 0..8 {
            for b in (a + 1)..8 {
                links.push(link(group * 8 + a + 1, group * 8 + b + 1, 10));
            }
        }
    }
    for group in 0..3i64 {
        links.push(link(group * 8 + 1, group * 8 + 9, 1));
    }
    (files, links, groups)
}

fn leaf_of(tree: &cluster::Tree) -> HashMap<&str, ComponentId> {
    tree.file_components
        .iter()
        .map(|(path, id)| (path.as_str(), *id))
        .collect()
}

#[test]
fn planted_communities_are_recovered_across_directories() {
    let (files, links, groups) = planted();

    let tree = compute(&files, &links, &Previous::default(), &Config::default());

    let leaf = leaf_of(&tree);
    let group_leaves: Vec<HashSet<ComponentId>> = groups
        .iter()
        .map(|paths| paths.iter().map(|p| leaf[p.as_str()]).collect())
        .collect();
    for leaves in &group_leaves {
        assert_eq!(leaves.len(), 1, "a planted community was split");
    }
    let distinct: HashSet<_> = group_leaves
        .iter()
        .map(|l| *l.iter().next().unwrap())
        .collect();
    assert_eq!(distinct.len(), 4, "planted communities were merged");
}

#[test]
fn every_file_is_in_one_leaf_and_the_tree_is_consistent() {
    let (files, links, _) = planted();

    let tree = compute(&files, &links, &Previous::default(), &Config::default());

    assert_eq!(tree.file_components.len(), files.len());
    let ids: HashSet<_> = tree.components.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), tree.components.len(), "ids are unique");
    let level_of: HashMap<_, _> = tree.components.iter().map(|c| (c.id, c.level)).collect();
    for component in &tree.components {
        match component.parent {
            None => assert_eq!(component.level, 0),
            Some(parent) => assert_eq!(component.level, level_of[&parent] + 1),
        }
        assert!(!component.stale);
    }
}

#[test]
fn directories_seed_files_that_have_no_links() {
    let files = vec![
        file(1, "net/a.rs"),
        file(2, "net/b.rs"),
        file(3, "net/c.rs"),
        file(4, "ui/a.rs"),
        file(5, "ui/b.rs"),
        file(6, "ui/c.rs"),
    ];

    let tree = compute(&files, &[], &Previous::default(), &Config::default());

    let leaf = leaf_of(&tree);
    assert_eq!(leaf["net/a.rs"], leaf["net/c.rs"]);
    assert_eq!(leaf["ui/a.rs"], leaf["ui/c.rs"]);
    assert_ne!(leaf["net/a.rs"], leaf["ui/a.rs"]);
    let names: HashSet<_> = tree.components.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains("net") && names.contains("ui"), "{names:?}");
}

#[test]
fn vendored_and_generated_files_are_excluded_unless_asked() {
    let mut files = vec![
        file(1, "src/a.rs"),
        file(2, "src/b.rs"),
        file(3, "third_party/x.rs"),
        file(4, "gen/y.rs"),
    ];
    files[2].tags.vendored = true;
    files[3].tags.generated = true;
    let links = [link(1, 3, 5)];

    let default = compute(&files, &links, &Previous::default(), &Config::default());
    let included = compute(
        &files,
        &links,
        &Previous::default(),
        &Config {
            include_vendored: true,
            ..Config::default()
        },
    );

    let paths: HashSet<_> = default
        .file_components
        .iter()
        .map(|(p, _)| p.as_str())
        .collect();
    assert_eq!(paths, HashSet::from(["src/a.rs", "src/b.rs"]));
    assert_eq!(included.file_components.len(), 4);
}

#[test]
fn unparsed_and_secret_files_are_never_clustered() {
    let mut files = vec![file(1, "a.rs"), file(2, "b.py"), file(3, ".env")];
    files[1].status = FileStatus::Unparsed;
    files[2].status = FileStatus::SkippedSecret;

    let tree = compute(&files, &[], &Previous::default(), &Config::default());

    assert_eq!(tree.file_components.len(), 1);
}

fn previous_of(tree: &cluster::Tree) -> Previous {
    Previous {
        components: tree.components.clone(),
        file_components: tree.file_components.clone(),
    }
}

#[test]
fn ids_survive_edits_by_member_overlap() {
    let (mut files, links, groups) = planted();
    let first = compute(&files, &links, &Previous::default(), &Config::default());
    let before = leaf_of(&first).clone();
    let before_ids: HashMap<String, ComponentId> =
        before.iter().map(|(p, id)| (p.to_string(), *id)).collect();

    // Edit one file's content and add a new file to the first planted community.
    files[0].content_hash = Some(ContentHash::of_bytes(b"edited"));
    files.push(file(100, "d0/g0_new.rs"));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut links = links;
    links.push(link(100, 1, 10));
    links.push(link(100, 2, 10));
    let second = compute(&files, &links, &previous_of(&first), &Config::default());

    let after = leaf_of(&second);
    for paths in &groups {
        for path in paths {
            assert_eq!(
                after[path.as_str()],
                before_ids[path],
                "{path} changed component id"
            );
        }
    }
    let new_ids: HashSet<_> = second.components.iter().map(|c| c.id).collect();
    let old_ids: HashSet<_> = first.components.iter().map(|c| c.id).collect();
    assert!(
        new_ids.is_subset(&old_ids),
        "an id was invented for an unchanged structure"
    );
}

#[test]
fn member_hash_changes_only_for_components_with_a_changed_file() {
    let (mut files, links, groups) = planted();
    let first = compute(&files, &links, &Previous::default(), &Config::default());
    let changed_path = groups[0][0].clone();
    let changed = files.iter_mut().find(|f| f.path == changed_path).unwrap();
    changed.content_hash = Some(ContentHash::of_bytes(b"edited"));

    let second = compute(&files, &links, &previous_of(&first), &Config::default());

    let hash_of = |tree: &cluster::Tree, id| {
        tree.components
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .member_hash
    };
    let leaf_first = leaf_of(&first);
    let touched = leaf_first[changed_path.as_str()];
    let untouched = leaf_first[groups[1][0].as_str()];
    assert_ne!(hash_of(&first, touched), hash_of(&second, touched));
    assert_eq!(hash_of(&first, untouched), hash_of(&second, untouched));
}

#[test]
fn staleness_names_components_with_changed_or_new_files() {
    let (mut files, links, groups) = planted();
    let tree = compute(&files, &links, &Previous::default(), &Config::default());
    let previous = previous_of(&tree);
    let config = Config::default();
    assert_eq!(
        check_staleness(&files, &previous, &config),
        cluster::Staleness::default()
    );

    let changed_path = groups[2][0].clone();
    files
        .iter_mut()
        .find(|f| f.path == changed_path)
        .unwrap()
        .content_hash = Some(ContentHash::of_bytes(b"edited"));
    files.push(file(100, "d9/brand_new.rs"));
    let staleness = check_staleness(&files, &previous, &config);

    let leaf = leaf_of(&tree);
    assert!(staleness.stale.contains(&leaf[changed_path.as_str()]));
    assert!(!staleness.stale.contains(&leaf[groups[0][0].as_str()]));
    assert_eq!(staleness.unassigned_files, 1);
}

#[test]
fn clustering_is_deterministic() {
    let (files, links, _) = planted();
    let a = compute(&files, &links, &Previous::default(), &Config::default());
    let b = compute(&files, &links, &Previous::default(), &Config::default());
    assert_eq!(a, b);
}

#[test]
fn an_empty_graph_gives_an_empty_tree() {
    let tree = compute(&[], &[], &Previous::default(), &Config::default());
    assert!(tree.components.is_empty() && tree.file_components.is_empty());
}
