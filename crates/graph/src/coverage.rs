//! Numbers for the `coverage` tool.

use crate::error::Result;
use crate::kinds::{EdgeKind, FileStatus, NodeKind, Provenance};
use crate::query::{CoverageInputs, EdgeCount, FileCount, ProblemFile};
use crate::rows::{count_column, decode, sql_limit};
use crate::store::Reader;

impl Reader {
    pub(crate) fn coverage_inputs(&self, problem_file_limit: usize) -> Result<CoverageInputs> {
        let parsed = FileStatus::Parsed.code();
        Ok(CoverageInputs {
            files: self.file_counts()?,
            edges: self.edge_counts()?,
            unresolved_edges: self.connection.query_row(
                // Unresolved nodes only receive edges, so their degrees add up to the edge count.
                &format!(
                    "SELECT COALESCE(sum(degree), 0) FROM nodes WHERE kind = {}",
                    NodeKind::Unresolved.code()
                ),
                [],
                |row| count_column(row, 0),
            )?,
            problem_files: self.problem_files(problem_file_limit)?,
            problem_file_total: self.connection.query_row(
                "SELECT count(*) FROM files WHERE status != ?1",
                [parsed],
                |row| count_column(row, 0),
            )?,
        })
    }

    fn file_counts(&self) -> Result<Vec<FileCount>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT language, status, count(*) FROM files GROUP BY language, status
             ORDER BY language, status",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(FileCount {
                language: row.get(0)?,
                status: decode(row.get(1)?, 1, FileStatus::from_code, "file status")?,
                count: count_column(row, 2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ponytail: scans every edge, seconds on Gecko; cache the counts in `meta` at load time if it shows up in a benchmark.
    fn edge_counts(&self) -> Result<Vec<EdgeCount>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT kind, provenance, count(*) FROM edges GROUP BY kind, provenance
             ORDER BY kind, provenance",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(EdgeCount {
                kind: decode(row.get(0)?, 0, EdgeKind::from_code, "edge kind")?,
                provenance: decode(row.get(1)?, 1, Provenance::from_code, "provenance")?,
                count: count_column(row, 2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn problem_files(&self, limit: usize) -> Result<Vec<ProblemFile>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT path, status, status_detail FROM files WHERE status != ?1 ORDER BY path LIMIT ?2",
        )?;
        let rows = statement.query_map((FileStatus::Parsed.code(), sql_limit(limit)), |row| {
            Ok(ProblemFile {
                path: row.get(0)?,
                status: decode(row.get(1)?, 1, FileStatus::from_code, "file status")?,
                detail: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
