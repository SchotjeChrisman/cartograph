//! The SQLite schema. Ids are integers; node ids are the stable symbol hashes.

use crate::kinds::NodeKind;

/// Bump when the tables change; older files are rejected, not migrated, until a release needs it.
pub const SCHEMA_VERSION: i64 = 2;

/// Connection settings applied to every connection. WAL lets readers run during a write.
pub const CONNECTION_PRAGMAS: &str = "
    PRAGMA busy_timeout = 10000;
    PRAGMA foreign_keys = OFF;
    PRAGMA temp_store = MEMORY;
";

/// Extra settings for the writer connection. The journal mode is persistent in the file.
pub const WRITER_PRAGMAS: &str = "
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    PRAGMA cache_size = -262144;
";

/// Secondary indexes on the two big tables. A first load builds them after the rows are in,
/// which is far cheaper than maintaining them row by row.
pub fn bulk_load_indexes() -> [BulkIndex; 6] {
    let unresolved = NodeKind::Unresolved.code();
    [
        BulkIndex::new("nodes_file", "nodes(file_id, line)"),
        BulkIndex::new("nodes_qualified_name", "nodes(qualified_name)"),
        BulkIndex {
            name: "nodes_unresolved",
            definition: format!("nodes(id) WHERE kind = {unresolved}"),
        },
        BulkIndex::new("edges_out", "edges(src, kind)"),
        BulkIndex::new("edges_in", "edges(dst, kind)"),
        BulkIndex::new("edges_file", "edges(file_id)"),
    ]
}

pub struct BulkIndex {
    pub name: &'static str,
    definition: String,
}

impl BulkIndex {
    fn new(name: &'static str, definition: &str) -> Self {
        BulkIndex {
            name,
            definition: definition.to_string(),
        }
    }

    pub fn create_statement(&self) -> String {
        format!("CREATE INDEX {} ON {}", self.name, self.definition)
    }
}

pub fn create_statements() -> String {
    let indexes: Vec<String> = bulk_load_indexes()
        .iter()
        .map(|index| format!("{};", index.create_statement()))
        .collect();
    format!("{TABLES}\n{}", indexes.join("\n"))
}

const TABLES: &str = "
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    repo TEXT NOT NULL,
    language TEXT,
    content_hash BLOB,
    size_bytes INTEGER NOT NULL,
    -- Modification time the content hash was taken at; NULL when the file was too fresh to trust.
    mtime_ns INTEGER,
    status INTEGER NOT NULL,
    status_detail TEXT,
    tags INTEGER NOT NULL,
    component_id INTEGER,
    indexed_at INTEGER NOT NULL
);
CREATE INDEX files_component ON files(component_id);

CREATE TABLE nodes (
    id INTEGER PRIMARY KEY,
    kind INTEGER NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    file_id INTEGER,
    line INTEGER,
    end_line INTEGER,
    signature TEXT,
    doc TEXT,
    degree INTEGER NOT NULL DEFAULT 0
);

CREATE VIRTUAL TABLE node_words USING fts5(words, prefix = '2 3');

CREATE TABLE edges (
    id INTEGER PRIMARY KEY,
    src INTEGER NOT NULL,
    dst INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    provenance INTEGER NOT NULL,
    file_id INTEGER NOT NULL,
    line INTEGER,
    reason TEXT
);

CREATE TABLE components (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER,
    level INTEGER NOT NULL,
    name TEXT NOT NULL,
    member_hash BLOB NOT NULL,
    stale INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX components_parent ON components(parent_id);

CREATE TABLE descriptions (
    component_id INTEGER PRIMARY KEY,
    component_hash BLOB NOT NULL,
    name TEXT NOT NULL,
    summary TEXT NOT NULL,
    tags TEXT NOT NULL,
    written_at INTEGER NOT NULL
);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    state INTEGER NOT NULL,
    owner TEXT NOT NULL,
    claimed_at INTEGER NOT NULL,
    finished_at INTEGER,
    detail TEXT,
    UNIQUE (kind, key)
);

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";
