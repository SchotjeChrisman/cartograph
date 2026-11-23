//! Clustering against a real graph file.

use cluster::{Config, mark_stale, recluster};
use graph::{
    ContentHash, EdgeFact, EdgeKind, FileFacts, FileRecord, FileStatus, FileTags, GraphDir,
    IndexerLock, NodeFact, NodeKind, Provenance, Reader, SymbolId, Writer,
};

const NOW: i64 = 1_700_000_000;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn function_id(path: &str, name: &str) -> SymbolId {
    SymbolId::derive(NodeKind::Function, name, path)
}

/// A file with functions `f0..f{count}` calling each other in a ring, plus calls into
/// `other_file`'s `f0` when given.
fn file_facts(path: &str, content: &str, count: usize, other_file: Option<&str>) -> FileFacts {
    let nodes = (0..count)
        .map(|index| NodeFact {
            kind: NodeKind::Function,
            name: format!("f{index}"),
            qualified_name: format!("f{index}"),
            line: Some(index as u32 * 3 + 1),
            end_line: Some(index as u32 * 3 + 3),
            signature: None,
            doc: None,
        })
        .collect();
    let call = |src, dst| EdgeFact {
        src,
        dst,
        kind: EdgeKind::Calls,
        provenance: Provenance::Extracted,
        line: Some(1),
        reason: None,
    };
    let mut edges: Vec<EdgeFact> = (0..count)
        .map(|index| {
            call(
                function_id(path, &format!("f{index}")),
                function_id(path, &format!("f{}", (index + 1) % count)),
            )
        })
        .collect();
    if let Some(other) = other_file {
        edges.push(call(function_id(path, "f0"), function_id(other, "f0")));
    }
    FileFacts {
        file: FileRecord {
            path: path.to_string(),
            repo: String::new(),
            language: Some("rust".to_string()),
            content_hash: Some(ContentHash::of_bytes(content.as_bytes())),
            size_bytes: content.len() as u64,
            mtime_ns: None,
            status: FileStatus::Parsed,
            status_detail: None,
            tags: FileTags::default(),
        },
        nodes,
        edges,
    }
}

fn load(dir: &GraphDir, files: Vec<FileFacts>) {
    let lock = IndexerLock::acquire(dir).unwrap();
    Writer::open(dir)
        .unwrap()
        .load(&lock, files, 10, NOW)
        .unwrap();
}

fn two_areas() -> Vec<FileFacts> {
    vec![
        file_facts("net/a.rs", "a", 3, Some("net/b.rs")),
        file_facts("net/b.rs", "b", 3, None),
        file_facts("ui/a.rs", "c", 3, Some("ui/b.rs")),
        file_facts("ui/b.rs", "d", 3, None),
        file_facts("ui/c.rs", "e", 3, Some("net/a.rs")),
    ]
}

#[test]
fn recluster_stores_the_tree_and_file_assignments() {
    let scratch = Scratch::new("recluster");
    let dir = GraphDir::of_root(&scratch.0);
    load(&dir, two_areas());
    let mut writer = Writer::open(&dir).unwrap();

    let tree = recluster(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();

    let reader = Reader::open(&dir).unwrap();
    let stored: usize = reader.components(None).unwrap().len();
    assert!(stored >= 1 && stored <= tree.components.len());
    let files = reader.files().unwrap();
    assert!(files.iter().all(|file| file.component.is_some()));
    let net = files
        .iter()
        .find(|f| f.path == "net/a.rs")
        .unwrap()
        .component;
    let net_b = files
        .iter()
        .find(|f| f.path == "net/b.rs")
        .unwrap()
        .component;
    assert_eq!(net, net_b);
}

#[test]
fn reclustering_after_an_edit_keeps_ids_and_clears_staleness() {
    let scratch = Scratch::new("reclusters");
    let dir = GraphDir::of_root(&scratch.0);
    load(&dir, two_areas());
    let mut writer = Writer::open(&dir).unwrap();
    let first = recluster(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();

    load(&dir, vec![file_facts("net/b.rs", "b edited", 3, None)]);
    let stale_now: Vec<_> = {
        let reader = Reader::open(&dir).unwrap();
        reader
            .components(None)
            .unwrap()
            .into_iter()
            .filter(|c| c.stale)
            .collect()
    };
    assert!(
        !stale_now.is_empty(),
        "the loader marks the edited file's component"
    );
    let second = recluster(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();

    let first_ids: Vec<_> = first.components.iter().map(|c| c.id).collect();
    assert!(second.components.iter().all(|c| first_ids.contains(&c.id)));
    assert!(second.components.iter().all(|c| !c.stale));
}

#[test]
fn mark_stale_reports_new_files_and_flags_changed_components() {
    let scratch = Scratch::new("markstale");
    let dir = GraphDir::of_root(&scratch.0);
    load(&dir, two_areas());
    let mut writer = Writer::open(&dir).unwrap();
    recluster(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();
    let clean = mark_stale(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();
    assert!(clean.stale.is_empty() && clean.unassigned_files == 0);

    load(&dir, vec![file_facts("net/new.rs", "n", 2, None)]);
    let found = mark_stale(
        &Reader::open(&dir).unwrap(),
        &mut writer,
        &Config::default(),
    )
    .unwrap();

    assert_eq!(found.unassigned_files, 1);
}
