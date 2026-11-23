//! Reading the file inventory: what the indexer compares hashes against and what cluster groups.

use serde::Serialize;

use crate::components::ComponentId;
use crate::error::Result;
use crate::facts::FileTags;
use crate::id::ContentHash;
use crate::kinds::{EdgeKind, FileStatus};
use crate::rows::{count_column, decode};
use crate::store::Reader;

/// One row of the files table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub repo: String,
    pub language: Option<String>,
    pub content_hash: Option<ContentHash>,
    pub size_bytes: u64,
    /// See [`crate::FileRecord::mtime_ns`].
    pub mtime_ns: Option<i64>,
    pub status: FileStatus,
    pub tags: FileTags,
    pub component: Option<ComponentId>,
}

/// How many non-containment edges run from one file to another; the input to clustering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLink {
    pub from_file: i64,
    pub to_file: i64,
    pub edge_count: u64,
}

impl Reader {
    /// Every file, in path order.
    pub fn files(&self) -> Result<Vec<FileRow>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id, path, repo, language, content_hash, status, tags, component_id, size_bytes, mtime_ns
             FROM files ORDER BY path",
        )?;
        let rows = statement.query_map([], |row| {
            let hash: Option<Vec<u8>> = row.get(4)?;
            let component: Option<i64> = row.get(7)?;
            Ok(FileRow {
                id: row.get(0)?,
                path: row.get(1)?,
                repo: row.get(2)?,
                language: row.get(3)?,
                content_hash: hash.as_deref().and_then(ContentHash::from_slice),
                size_bytes: count_column(row, 8)?,
                mtime_ns: row.get(9)?,
                status: decode(row.get(5)?, 5, FileStatus::from_code, "file status")?,
                tags: FileTags::from_bits(row.get(6)?),
                component: component.map(ComponentId),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Edges between different files, grouped by direction of the pair.
    pub fn file_links(&self) -> Result<Vec<FileLink>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT src_node.file_id, dst_node.file_id, count(*)
             FROM edges JOIN nodes src_node ON src_node.id = edges.src
                        JOIN nodes dst_node ON dst_node.id = edges.dst
             WHERE edges.kind != ?1 AND src_node.file_id IS NOT NULL AND dst_node.file_id IS NOT NULL
               AND src_node.file_id != dst_node.file_id
             GROUP BY src_node.file_id, dst_node.file_id",
        )?;
        let rows = statement.query_map([EdgeKind::Contains.code()], |row| {
            Ok(FileLink {
                from_file: row.get(0)?,
                to_file: row.get(1)?,
                edge_count: count_column(row, 2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
