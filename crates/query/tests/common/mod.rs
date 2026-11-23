//! A hand-built graph shared by the golden tests.
//!
//! ```text
//! app::main -> app::run -> net::open, net::Socket::connect, ui::open
//! ui::Window::draw -> net::open;  ui::open -> ui::Window::draw
//! net::Socket::connect -> net::open, (inferred) net::read_all, (unresolved) missing_fn
//! net::read_all -> zlib::inflate (vendored), (unresolved) libc_read;  zlib::open is vendored
//! ```
//! Components: #1 Application > #2 Networking (src/net), #3 Interface (src/ui) and #4 Core (src/app).
#![allow(dead_code)]

use std::path::PathBuf;

use graph::{
    Component, ComponentId, ContentHash, Description, EdgeFact, EdgeKind, FileFacts, FileRecord,
    FileStatus, FileTags, GraphDir, IndexerLock, NodeFact, NodeKind, Provenance, Reader, SymbolId,
    Writer,
};

pub const INDEXED_AT: i64 = 1_700_000_000;
pub const NOW: i64 = INDEXED_AT + 2 * 3600;

/// A scratch directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        TestDir(path)
    }

    pub fn graph_dir(&self) -> GraphDir {
        GraphDir::of_root(&self.0)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn record(path: &str, language: Option<&str>, status: FileStatus, tags: FileTags) -> FileRecord {
    FileRecord {
        path: path.to_string(),
        repo: String::new(),
        language: language.map(str::to_string),
        content_hash: Some(ContentHash::of_bytes(path.as_bytes())),
        size_bytes: 100,
        mtime_ns: None,
        status,
        status_detail: None,
        tags,
    }
}

fn node(kind: NodeKind, qualified_name: &str, line: u32) -> NodeFact {
    NodeFact {
        kind,
        name: qualified_name
            .rsplit("::")
            .next()
            .unwrap_or(qualified_name)
            .to_string(),
        qualified_name: qualified_name.to_string(),
        line: Some(line),
        end_line: Some(line + 4),
        signature: None,
        doc: None,
    }
}

fn function(qualified_name: &str, line: u32) -> NodeFact {
    node(NodeKind::Function, qualified_name, line)
}

fn method(qualified_name: &str, line: u32) -> NodeFact {
    node(NodeKind::Method, qualified_name, line)
}

pub fn id(path: &str, kind: NodeKind, qualified_name: &str) -> SymbolId {
    SymbolId::derive(kind, qualified_name, path)
}

fn edge(src: SymbolId, dst: SymbolId, kind: EdgeKind, line: u32) -> EdgeFact {
    EdgeFact {
        src,
        dst,
        kind,
        provenance: Provenance::Extracted,
        line: Some(line),
        reason: None,
    }
}

fn inferred(mut edge: EdgeFact, reason: &str) -> EdgeFact {
    edge.provenance = Provenance::Inferred;
    edge.reason = Some(reason.to_string());
    edge
}

fn unresolved(name: &str) -> (NodeFact, SymbolId) {
    let fact = NodeFact {
        line: None,
        end_line: None,
        ..node(NodeKind::Unresolved, name, 0)
    };
    (fact, SymbolId::derive(NodeKind::Unresolved, name, ""))
}

const SOCKET: &str = "src/net/socket.rs";
const MAIN: &str = "src/app/main.rs";
const RUN: &str = "src/app/run.rs";
const WINDOW: &str = "src/ui/window.rs";
const INFLATE: &str = "vendor/zlib/inflate.c";

fn fn_id(path: &str, name: &str) -> SymbolId {
    id(path, NodeKind::Function, name)
}

fn method_id(path: &str, name: &str) -> SymbolId {
    id(path, NodeKind::Method, name)
}

pub fn fixture_facts() -> Vec<FileFacts> {
    let (missing, missing_id) = unresolved("missing_fn");
    let (libc_read, libc_read_id) = unresolved("libc_read");
    let open = fn_id(SOCKET, "net::open");
    let connect = method_id(SOCKET, "net::Socket::connect");
    let read_all = fn_id(SOCKET, "net::read_all");
    let draw = method_id(WINDOW, "ui::Window::draw");
    let ui_open = fn_id(WINDOW, "ui::open");
    let run = fn_id(RUN, "app::run");
    let inflate = fn_id(INFLATE, "zlib::inflate");
    let socket_file = id(SOCKET, NodeKind::File, SOCKET);

    let socket = FileFacts {
        file: record(
            SOCKET,
            Some("rust"),
            FileStatus::Parsed,
            FileTags::default(),
        ),
        nodes: vec![
            node(NodeKind::File, SOCKET, 1),
            function("net::open", 10),
            method("net::Socket::connect", 30),
            function("net::read_all", 50),
            missing,
            libc_read,
        ],
        edges: vec![
            edge(socket_file, open, EdgeKind::Contains, 10),
            edge(socket_file, connect, EdgeKind::Contains, 30),
            edge(socket_file, read_all, EdgeKind::Contains, 50),
            edge(connect, open, EdgeKind::Calls, 32),
            inferred(
                edge(connect, read_all, EdgeKind::Calls, 34),
                "same-name function in scope",
            ),
            edge(connect, missing_id, EdgeKind::Calls, 35),
            edge(read_all, inflate, EdgeKind::Calls, 52),
            edge(read_all, libc_read_id, EdgeKind::Calls, 53),
        ],
    };
    let main = FileFacts {
        file: record(MAIN, Some("rust"), FileStatus::Parsed, FileTags::default()),
        nodes: vec![function("app::main", 5)],
        edges: vec![edge(fn_id(MAIN, "app::main"), run, EdgeKind::Calls, 6)],
    };
    let run_file = FileFacts {
        file: record(RUN, Some("rust"), FileStatus::Parsed, FileTags::default()),
        nodes: vec![function("app::run", 3)],
        edges: vec![
            edge(run, open, EdgeKind::Calls, 4),
            edge(run, connect, EdgeKind::Calls, 5),
            edge(run, ui_open, EdgeKind::Calls, 6),
        ],
    };
    let window = FileFacts {
        file: record(
            WINDOW,
            Some("rust"),
            FileStatus::Parsed,
            FileTags::default(),
        ),
        nodes: vec![function("ui::open", 8), method("ui::Window::draw", 20)],
        edges: vec![
            edge(ui_open, draw, EdgeKind::Calls, 9),
            edge(draw, open, EdgeKind::Calls, 22),
        ],
    };
    let vendored = FileFacts {
        file: record(
            INFLATE,
            Some("c"),
            FileStatus::Parsed,
            FileTags {
                vendored: true,
                generated: false,
            },
        ),
        nodes: vec![function("zlib::inflate", 100), function("zlib::open", 200)],
        edges: Vec::new(),
    };
    let script = FileFacts {
        file: record(
            "scripts/build.py",
            Some("python"),
            FileStatus::Unparsed,
            FileTags::default(),
        ),
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    let mut broken_record = record(
        "src/broken.rs",
        Some("rust"),
        FileStatus::ParseFailed,
        FileTags::default(),
    );
    broken_record.status_detail = Some("syntax error at line 3".to_string());
    let broken = FileFacts {
        file: broken_record,
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    vec![socket, main, run_file, window, vendored, script, broken]
}

fn component(id: i64, parent: Option<i64>, level: u32, name: &str) -> Component {
    Component {
        id: ComponentId(id),
        parent: parent.map(ComponentId),
        level,
        name: name.to_string(),
        member_hash: ContentHash::of_bytes(name.as_bytes()),
        stale: false,
    }
}

fn description(
    component: &Component,
    name: &str,
    summary: &str,
    tags: &[&str],
    hash: &[u8],
) -> Description {
    Description {
        component: component.id,
        component_hash: ContentHash::of_bytes(hash),
        name: name.to_string(),
        summary: summary.to_string(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        written_at: INDEXED_AT,
    }
}

/// Loads the fixture, its components and descriptions, and returns a reader over it.
pub fn load(dir: &GraphDir) -> Reader {
    let lock = IndexerLock::acquire(dir).unwrap();
    let mut writer = Writer::open(dir).unwrap();
    writer.load(&lock, fixture_facts(), 10, INDEXED_AT).unwrap();
    let components = vec![
        component(1, None, 0, "app"),
        component(2, Some(1), 1, "net"),
        component(3, Some(1), 1, "ui"),
        component(4, Some(1), 1, "core"),
    ];
    let assignments = [(SOCKET, 2), (WINDOW, 3), (MAIN, 4), (RUN, 4)]
        .map(|(path, id)| (path.to_string(), ComponentId(id)));
    writer
        .replace_components(&components, &assignments)
        .unwrap();
    writer
        .put_description(&description(
            &components[0],
            "Application",
            "Connects to a server and shows what it sends. Everything else hangs off it.",
            &["app"],
            b"app",
        ))
        .unwrap();
    writer
        .put_description(&description(
            &components[1],
            "Networking",
            "Sockets and reading from them.",
            &["io", "net"],
            b"net",
        ))
        .unwrap();
    writer
        .put_description(&description(
            &components[2],
            "Interface",
            "Windows and drawing.",
            &[],
            b"older ui",
        ))
        .unwrap();
    drop(lock);
    Reader::open(dir).unwrap()
}

/// Compares `actual` with `tests/golden/<name>.txt`; `UPDATE_GOLDEN=1` rewrites the file.
pub fn assert_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()));
    assert_eq!(
        actual, expected,
        "golden {name} differs; rerun with UPDATE_GOLDEN=1 to accept"
    );
}
