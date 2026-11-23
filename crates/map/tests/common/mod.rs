//! A small graph on disk and a server over it, shared by the tests and the browser-check example.

use std::path::PathBuf;

use graph::{
    Component, ComponentId, ContentHash, Description, EdgeFact, EdgeKind, FileFacts, FileRecord,
    FileStatus, FileTags, GraphDir, IndexerLock, NodeFact, NodeKind, Provenance, SymbolId, Writer,
};

pub const NOW: i64 = 1_700_000_000;
pub const REPLY: &str =
    r#"{"name": "Storage layer", "paragraph": "Keeps data on disk.", "tags": ["io"]}"#;

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let base = std::env::var_os("TEST_SCRATCH").map_or_else(std::env::temp_dir, PathBuf::from);
        let path = base.join(format!("map-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    pub fn dir(&self) -> GraphDir {
        GraphDir::of_root(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn qualified(path: &str, name: &str) -> String {
    format!("{}::{name}", path.replace(['/', '.'], "_"))
}

fn function_id(path: &str, name: &str) -> SymbolId {
    SymbolId::derive(NodeKind::Function, &qualified(path, name), path)
}

/// A file with functions `f0..f{count}` in a ring, plus one call into `calls`'s `f0`.
fn file(path: &str, functions: usize, calls: &[&str]) -> FileFacts {
    let call = |src, dst| EdgeFact {
        src,
        dst,
        kind: EdgeKind::Calls,
        provenance: Provenance::Extracted,
        line: Some(1),
        reason: None,
    };
    let mut edges: Vec<EdgeFact> = (0..functions)
        .map(|index| {
            call(
                function_id(path, &format!("f{index}")),
                function_id(path, &format!("f{}", (index + 1) % functions)),
            )
        })
        .collect();
    edges.extend(
        calls
            .iter()
            .map(|other| call(function_id(path, "f0"), function_id(other, "f0"))),
    );
    FileFacts {
        file: FileRecord {
            path: path.to_string(),
            repo: String::new(),
            language: Some("rust".to_string()),
            content_hash: Some(ContentHash::of_bytes(path.as_bytes())),
            size_bytes: 10,
            mtime_ns: None,
            status: FileStatus::Parsed,
            status_detail: None,
            tags: FileTags::default(),
        },
        nodes: (0..functions)
            .map(|index| NodeFact {
                kind: NodeKind::Function,
                name: format!("f{index}"),
                qualified_name: qualified(path, &format!("f{index}")),
                line: Some(index as u32 * 3 + 1),
                end_line: Some(index as u32 * 3 + 3),
                signature: None,
                doc: None,
            })
            .collect(),
        edges,
    }
}

fn component(id: i64, parent: Option<i64>, level: u32, name: &str, stale: bool) -> Component {
    Component {
        id: ComponentId(id),
        parent: parent.map(ComponentId),
        level,
        name: name.to_string(),
        member_hash: ContentHash::of_bytes(name.as_bytes()),
        stale,
    }
}

/// Ids: 1 `app` (2 `net`, 3 `ui`, 4 `storage`, stale), 5 `tools`. `ui` calls `net` and
/// `storage`, `net` calls `storage`. Only `net` has a description.
pub fn write_fixture(dir: &GraphDir) {
    let files = vec![
        file("net/a.rs", 3, &["storage/a.rs"]),
        file("net/b.rs", 2, &[]),
        file("ui/a.rs", 3, &["net/a.rs", "net/b.rs", "storage/a.rs"]),
        file("storage/a.rs", 3, &[]),
        file("tools/a.rs", 2, &[]),
    ];
    let lock = IndexerLock::acquire(dir).unwrap();
    let mut writer = Writer::open(dir).unwrap();
    writer.load(&lock, files, 10, NOW).unwrap();
    let components = vec![
        component(1, None, 0, "app", false),
        component(2, Some(1), 1, "net", false),
        component(3, Some(1), 1, "ui", false),
        component(4, Some(1), 1, "storage", true),
        component(5, None, 0, "tools", false),
    ];
    let assignments: Vec<(String, ComponentId)> = [
        ("net/a.rs", 2),
        ("net/b.rs", 2),
        ("ui/a.rs", 3),
        ("storage/a.rs", 4),
        ("tools/a.rs", 5),
    ]
    .into_iter()
    .map(|(path, id)| (path.to_string(), ComponentId(id)))
    .collect();
    writer
        .replace_components(&components, &assignments)
        .unwrap();
    writer
        .put_description(&Description {
            component: ComponentId(2),
            component_hash: components[1].member_hash,
            name: "Networking".to_string(),
            summary: "Talks to the wire.".to_string(),
            tags: vec!["net".to_string()],
            written_at: NOW,
        })
        .unwrap();
}
