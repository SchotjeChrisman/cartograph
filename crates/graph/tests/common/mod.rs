//! Builders shared by the integration tests.
#![allow(dead_code)]

use std::path::PathBuf;

use graph::{
    ContentHash, EdgeFact, EdgeKind, FileFacts, FileRecord, FileStatus, FileTags, GraphDir,
    NodeFact, NodeKind, Provenance, SymbolId,
};

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

pub fn file_record(path: &str, content: &str) -> FileRecord {
    FileRecord {
        path: path.to_string(),
        repo: String::new(),
        language: Some("rust".to_string()),
        content_hash: Some(ContentHash::of_bytes(content.as_bytes())),
        size_bytes: content.len() as u64,
        mtime_ns: None,
        status: FileStatus::Parsed,
        status_detail: None,
        tags: FileTags::default(),
    }
}

pub fn node(kind: NodeKind, qualified_name: &str, line: u32) -> NodeFact {
    NodeFact {
        kind,
        name: qualified_name
            .rsplit("::")
            .next()
            .unwrap_or(qualified_name)
            .to_string(),
        qualified_name: qualified_name.to_string(),
        line: Some(line),
        end_line: Some(line + 2),
        signature: None,
        doc: None,
    }
}

pub fn function(qualified_name: &str, line: u32) -> NodeFact {
    node(NodeKind::Function, qualified_name, line)
}

pub fn function_id(path: &str, qualified_name: &str) -> SymbolId {
    SymbolId::derive(NodeKind::Function, qualified_name, path)
}

pub fn edge(src: SymbolId, dst: SymbolId, kind: EdgeKind) -> EdgeFact {
    EdgeFact {
        src,
        dst,
        kind,
        provenance: Provenance::Extracted,
        line: Some(1),
        reason: None,
    }
}

pub fn call(src: SymbolId, dst: SymbolId) -> EdgeFact {
    edge(src, dst, EdgeKind::Calls)
}

pub fn facts(path: &str, nodes: Vec<NodeFact>, edges: Vec<EdgeFact>) -> FileFacts {
    FileFacts {
        file: file_record(path, path),
        nodes,
        edges,
    }
}

/// A chain of functions `prefix0 -> prefix1 -> ...` in one file.
pub fn chain_file(path: &str, prefix: &str, length: u32) -> FileFacts {
    let names: Vec<String> = (0..length)
        .map(|index| format!("{prefix}{index}"))
        .collect();
    let nodes = names
        .iter()
        .enumerate()
        .map(|(line, name)| function(name, line as u32 * 3))
        .collect();
    let edges = names
        .windows(2)
        .map(|pair| call(function_id(path, &pair[0]), function_id(path, &pair[1])))
        .collect();
    facts(path, nodes, edges)
}
