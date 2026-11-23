//! What the indexer hands to the loader: everything learned from one file.

use serde::Serialize;

use crate::id::{ContentHash, SymbolId};
use crate::kinds::{EdgeKind, FileStatus, NodeKind, Provenance};

/// Path-based flags that de-rank a file in queries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FileTags {
    pub vendored: bool,
    pub generated: bool,
}

impl FileTags {
    const VENDORED_BIT: i64 = 1;
    const GENERATED_BIT: i64 = 2;

    pub fn to_bits(self) -> i64 {
        (if self.vendored { Self::VENDORED_BIT } else { 0 })
            | (if self.generated {
                Self::GENERATED_BIT
            } else {
                0
            })
    }

    pub fn from_bits(bits: i64) -> Self {
        FileTags {
            vendored: bits & Self::VENDORED_BIT != 0,
            generated: bits & Self::GENERATED_BIT != 0,
        }
    }
}

/// One row of the files table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    /// Path relative to the graph root, with `/` separators.
    pub path: String,
    /// Path of the nearest `.git` parent relative to the root; empty when there is none.
    pub repo: String,
    pub language: Option<String>,
    /// `None` for files whose content must not be read, such as secrets.
    pub content_hash: Option<ContentHash>,
    pub size_bytes: u64,
    /// Modification time (ns since the epoch) when the hash was taken, so a later run can skip
    /// reading a file whose size and time are unchanged. `None` when the file was modified so
    /// recently that an edit could still share that time.
    pub mtime_ns: Option<i64>,
    pub status: FileStatus,
    /// Why the file was skipped or failed; shown by coverage.
    pub status_detail: Option<String>,
    pub tags: FileTags,
}

/// A node found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFact {
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub signature: Option<String>,
    pub doc: Option<String>,
}

/// An edge found in a file; both ends are ids, so an end may live in a file not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeFact {
    pub src: SymbolId,
    pub dst: SymbolId,
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub line: Option<u32>,
    /// Required for `inferred` edges.
    pub reason: Option<String>,
}

/// Everything one file contributes. Loading it replaces whatever the file contributed before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    pub file: FileRecord,
    pub nodes: Vec<NodeFact>,
    pub edges: Vec<EdgeFact>,
}

impl NodeFact {
    /// The node's id when found in `file_path`.
    pub fn id_in(&self, file_path: &str) -> SymbolId {
        SymbolId::derive(self.kind, &self.qualified_name, file_path)
    }
}

impl EdgeFact {
    /// Checks the rules the graph model puts on every edge.
    pub fn validate(&self) -> crate::Result<()> {
        let missing_reason = self.provenance == Provenance::Inferred
            && self.reason.as_deref().is_none_or(str::is_empty);
        if missing_reason {
            return Err(crate::Error::InvalidFact(format!(
                "inferred {} edge {} -> {} has no reason",
                self.kind, self.src, self.dst
            )));
        }
        Ok(())
    }
}
