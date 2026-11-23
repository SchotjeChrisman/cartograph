//! The file-backed implementation of [`Query`].

use std::cmp::Reverse;

use rusqlite::{OptionalExtension, Row};

use crate::components::{Component, ComponentId};
use crate::csr::Direction;
use crate::error::Result;
use crate::id::SymbolId;
use crate::kinds::{EdgeKind, NodeKind, Provenance};
use crate::names::split_words;
use crate::query::{
    CoverageInputs, EdgeInfo, Freshness, Hop, Impact, Neighbour, Neighbours, NodeInfo, PathStep,
    Query, Reached,
};
use crate::rows::{
    NODE_COLUMN_COUNT, NODE_COLUMNS, count_column, decode, edge_columns, kind_filter,
    node_from_row, sql_limit,
};
use crate::slice::SLICE_NODE_CAP;
use crate::store::Reader;

/// Fuzzy candidates fetched per requested result, so re-ranking by degree has something to choose from.
const FUZZY_POOL_FACTOR: usize = 8;
const SYMBOL_KINDS: [NodeKind; 4] = [
    NodeKind::Function,
    NodeKind::Method,
    NodeKind::Class,
    NodeKind::Type,
];
const INDEXED_AT_KEY: &str = "indexed_at";

impl Query for Reader {
    fn find(&self, text: &str, limit: usize) -> Result<Vec<NodeInfo>> {
        let text = text.trim();
        if let Some(prefix) = text.strip_prefix('@') {
            return self.find_by_id_prefix(prefix, limit);
        }
        for found in [
            self.find_at_location(text, limit)?,
            self.find_by_qualified_name(text, limit)?,
        ] {
            if !found.is_empty() {
                return Ok(found);
            }
        }
        self.find_fuzzy(text, limit)
    }

    fn node(&self, id: SymbolId) -> Result<Option<NodeInfo>> {
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN files f ON f.id = n.file_id WHERE n.id = ?1"
        );
        let found = self
            .connection
            .prepare_cached(&sql)?
            .query_row([id.as_i64()], |row| node_from_row(row, 0))
            .optional()?;
        Ok(found)
    }

    fn neighbours(
        &self,
        id: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        limit: usize,
    ) -> Result<Neighbours> {
        self.in_snapshot(|| {
            let mut items = Vec::new();
            let mut total = 0;
            for followed in direction.directions() {
                let (found, count) = self.neighbours_one_way(id, *followed, kinds, limit)?;
                items.extend(found);
                total += count;
            }
            items.sort_by_key(|neighbour| Reverse(neighbour.node.degree));
            items.truncate(limit);
            Ok(Neighbours { items, total })
        })
    }

    fn path(
        &self,
        from: SymbolId,
        to: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        max_depth: u32,
    ) -> Result<Option<Vec<Hop>>> {
        self.in_snapshot(|| self.path_in_snapshot(from, to, direction, kinds, max_depth))
    }

    fn impact(
        &self,
        id: SymbolId,
        kinds: &[EdgeKind],
        max_depth: u32,
        limit: usize,
    ) -> Result<Impact> {
        self.in_snapshot(|| self.impact_in_snapshot(id, kinds, max_depth, limit))
    }

    fn coverage(&self, problem_file_limit: usize) -> Result<CoverageInputs> {
        self.coverage_inputs(problem_file_limit)
    }

    fn freshness(&self) -> Result<Freshness> {
        let indexed_at: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [INDEXED_AT_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(Freshness {
            indexed_at: indexed_at.and_then(|value| value.parse().ok()),
            indexing_in_progress: crate::lock::IndexerLock::is_held(&self.dir)?,
        })
    }

    fn components(&self, parent: Option<ComponentId>) -> Result<Vec<Component>> {
        Reader::components(self, parent)
    }

    fn top_symbols(&self, component: ComponentId, limit: usize) -> Result<Vec<NodeInfo>> {
        let kinds: Vec<String> = SYMBOL_KINDS
            .iter()
            .map(|kind| kind.code().to_string())
            .collect();
        let sql = format!(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT components.id FROM components JOIN subtree ON components.parent_id = subtree.id
             )
             SELECT {NODE_COLUMNS} FROM files f JOIN nodes n ON n.file_id = f.id
             WHERE f.component_id IN (SELECT id FROM subtree) AND n.kind IN ({})
             ORDER BY n.degree DESC, n.id LIMIT ?2",
            kinds.join(",")
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows =
            statement.query_map((component.0, sql_limit(limit)), |row| node_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

impl Reader {
    /// The signature and doc comment the indexer stored for a symbol; both are `None` for a
    /// symbol without them or an unknown id. Kept apart from [`NodeInfo`], which every query
    /// answer carries.
    pub fn symbol_text(&self, id: SymbolId) -> Result<(Option<String>, Option<String>)> {
        let text = self
            .connection
            .prepare_cached("SELECT signature, doc FROM nodes WHERE id = ?1")?
            .query_row([id.as_i64()], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        Ok(text.unwrap_or_default())
    }

    /// Runs several reads against one committed snapshot, so a batch the indexer commits midway
    /// cannot make the results disagree.
    fn in_snapshot<T>(&self, reads: impl FnOnce() -> Result<T>) -> Result<T> {
        let transaction = self.connection.unchecked_transaction()?;
        let result = reads()?;
        transaction.commit()?;
        Ok(result)
    }

    fn path_in_snapshot(
        &self,
        from: SymbolId,
        to: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        max_depth: u32,
    ) -> Result<Option<Vec<Hop>>> {
        if from == to {
            return Ok(self.node(from)?.map(|node| vec![Hop { node, via: None }]));
        }
        let slice = self.load_slice(from, direction, kinds, max_depth, SLICE_NODE_CAP)?;
        let (Some(start), Some(goal)) = (slice.local(from), slice.local(to)) else {
            return Ok(None);
        };
        let Some(visits) = slice.shortest_path(start, goal, direction, max_depth) else {
            return Ok(None);
        };
        let mut hops = Vec::with_capacity(visits.len());
        for visit in visits {
            let Some(node) = self.node(slice.id(visit.node))? else {
                return Ok(None);
            };
            let via = visit.parent.map(|(_, link, forward)| PathStep {
                kind: link.kind,
                provenance: link.provenance,
                line: link.line,
                forward,
            });
            hops.push(Hop { node, via });
        }
        Ok(Some(hops))
    }

    fn impact_in_snapshot(
        &self,
        id: SymbolId,
        kinds: &[EdgeKind],
        max_depth: u32,
        limit: usize,
    ) -> Result<Impact> {
        let slice = self.load_slice(id, Direction::In, kinds, max_depth, SLICE_NODE_CAP)?;
        let Some(start) = slice.local(id) else {
            return Ok(Impact {
                reached: Vec::new(),
                total: 0,
                truncated: false,
            });
        };
        let visits = slice.reach(start, Direction::In, max_depth);
        let mut reached = Vec::new();
        for visit in visits.iter().skip(1).take(limit) {
            if let Some(node) = self.node(slice.id(visit.node))? {
                reached.push(Reached {
                    node,
                    depth: visit.depth,
                });
            }
        }
        Ok(Impact {
            reached,
            total: visits.len() - 1,
            truncated: slice.is_truncated(),
        })
    }

    fn find_by_id_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<NodeInfo>> {
        let Some((low, high)) = SymbolId::range_for_prefix(prefix) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN files f ON f.id = n.file_id
             WHERE n.id BETWEEN ?1 AND ?2 ORDER BY n.id LIMIT ?3"
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows =
            statement.query_map((low, high, sql_limit(limit)), |row| node_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Resolves `path:line` to the smallest definition enclosing that line.
    fn find_at_location(&self, text: &str, limit: usize) -> Result<Vec<NodeInfo>> {
        let Some((path, line)) = text.rsplit_once(':') else {
            return Ok(Vec::new());
        };
        let Ok(line) = line.parse::<u32>() else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM files f JOIN nodes n ON n.file_id = f.id
             WHERE f.path = ?1 AND n.line <= ?2 AND COALESCE(n.end_line, n.line) >= ?2
             ORDER BY COALESCE(n.end_line, n.line) - n.line, n.degree DESC LIMIT ?3"
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows =
            statement.query_map((path, line, sql_limit(limit)), |row| node_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn find_by_qualified_name(&self, text: &str, limit: usize) -> Result<Vec<NodeInfo>> {
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN files f ON f.id = n.file_id
             WHERE n.qualified_name = ?1 AND n.kind != {} ORDER BY n.degree DESC LIMIT ?2",
            NodeKind::Unresolved.code()
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map((text, sql_limit(limit)), |row| node_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Word-prefix search over qualified names. Exact names first, vendored and generated code
    /// last, then best text match, then most connected.
    fn find_fuzzy(&self, text: &str, limit: usize) -> Result<Vec<NodeInfo>> {
        let words = split_words(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let expression: Vec<String> = words.iter().map(|word| format!("\"{word}\"*")).collect();
        let sql = format!(
            "SELECT {NODE_COLUMNS}, node_words.rank FROM node_words
             JOIN nodes n ON n.id = node_words.rowid LEFT JOIN files f ON f.id = n.file_id
             WHERE node_words MATCH ?1 AND n.kind != {} ORDER BY node_words.rank LIMIT ?2",
            NodeKind::Unresolved.code()
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map(
            (
                expression.join(" AND "),
                sql_limit(limit.saturating_mul(FUZZY_POOL_FACTOR)),
            ),
            |row| {
                Ok((
                    node_from_row(row, 0)?,
                    row.get::<_, f64>(NODE_COLUMN_COUNT)?,
                ))
            },
        )?;
        let mut candidates: Vec<(NodeInfo, f64)> = rows.collect::<rusqlite::Result<_>>()?;
        candidates.sort_by(|(first, first_rank), (second, second_rank)| {
            let key = |node: &NodeInfo| {
                let exact = split_words(&node.name) == words;
                (node.tags.vendored || node.tags.generated, !exact)
            };
            key(first)
                .cmp(&key(second))
                .then(first_rank.total_cmp(second_rank))
                .then(second.degree.cmp(&first.degree))
        });
        Ok(candidates
            .into_iter()
            .take(limit)
            .map(|(node, _)| node)
            .collect())
    }

    fn neighbours_one_way(
        &self,
        id: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        limit: usize,
    ) -> Result<(Vec<Neighbour>, usize)> {
        let (own, other) = edge_columns(direction);
        let filter = kind_filter("e.kind", kinds);
        let sql = format!(
            "SELECT {NODE_COLUMNS}, e.kind, e.provenance, ef.path, e.line, e.reason
             FROM edges e JOIN nodes n ON n.id = e.{other} LEFT JOIN files f ON f.id = n.file_id
             JOIN files ef ON ef.id = e.file_id
             WHERE e.{own} = ?1{filter} ORDER BY n.degree DESC, e.id LIMIT ?2"
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map((id.as_i64(), sql_limit(limit)), |row| {
            neighbour_from_row(row, direction)
        })?;
        let items = rows.collect::<rusqlite::Result<_>>()?;
        let total = self.connection.query_row(
            &format!("SELECT count(*) FROM edges e WHERE e.{own} = ?1{filter}"),
            [id.as_i64()],
            |row| count_column(row, 0),
        )?;
        Ok((items, total as usize))
    }
}

fn neighbour_from_row(row: &Row, direction: Direction) -> rusqlite::Result<Neighbour> {
    let first = NODE_COLUMN_COUNT;
    Ok(Neighbour {
        node: node_from_row(row, 0)?,
        direction,
        edge: EdgeInfo {
            kind: decode(row.get(first)?, first, EdgeKind::from_code, "edge kind")?,
            provenance: decode(
                row.get(first + 1)?,
                first + 1,
                Provenance::from_code,
                "provenance",
            )?,
            file: row.get(first + 2)?,
            line: row.get(first + 3)?,
            reason: row.get(first + 4)?,
        },
    })
}
