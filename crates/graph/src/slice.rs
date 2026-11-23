//! Loading a bounded neighbourhood of one node into a [`Csr`].

use std::collections::HashSet;

use crate::csr::{Csr, Direction, SliceEdge};
use crate::error::Result;
use crate::id::SymbolId;
use crate::kinds::{EdgeKind, Provenance};
use crate::rows::{decode, edge_columns, kind_filter, symbol_id};
use crate::store::Reader;

/// Most nodes a slice may hold, so one query cannot exhaust memory on a huge graph.
pub const SLICE_NODE_CAP: usize = 200_000;

impl Reader {
    /// Walks outward from `seed` one level at a time, fetching each level's edges from SQLite,
    /// until `max_depth` or `node_cap` nodes. The traversal itself then runs on the result.
    pub fn load_slice(
        &self,
        seed: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        max_depth: u32,
        node_cap: usize,
    ) -> Result<Csr> {
        let mut seen = HashSet::from([seed]);
        let mut seen_edges = HashSet::new();
        let mut edges = Vec::new();
        let mut frontier = vec![seed];
        let mut truncated = false;
        for _ in 0..max_depth {
            let mut next = Vec::new();
            for node in &frontier {
                for followed in direction.directions() {
                    for (edge_id, edge) in self.edges_of(*node, *followed, kinds)? {
                        let other = if *followed == Direction::Out {
                            edge.dst
                        } else {
                            edge.src
                        };
                        let known = seen.contains(&other);
                        if !known && seen.len() >= node_cap {
                            truncated = true;
                            continue;
                        }
                        if seen_edges.insert(edge_id) {
                            edges.push(edge);
                        }
                        if seen.insert(other) {
                            next.push(other);
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(Csr::build(&edges, truncated))
    }

    fn edges_of(
        &self,
        node: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
    ) -> Result<Vec<(i64, SliceEdge)>> {
        let (own, _) = edge_columns(direction);
        let sql = format!(
            "SELECT id, src, dst, kind, provenance, line FROM edges WHERE {own} = ?1{}",
            kind_filter("kind", kinds)
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map([node.as_i64()], |row| {
            let src: i64 = row.get(1)?;
            let dst: i64 = row.get(2)?;
            Ok((
                row.get::<_, i64>(0)?,
                SliceEdge {
                    src: symbol_id(src, 1)?,
                    dst: symbol_id(dst, 2)?,
                    kind: decode(row.get(3)?, 3, EdgeKind::from_code, "edge kind")?,
                    provenance: decode(row.get(4)?, 4, Provenance::from_code, "provenance")?,
                    line: row.get(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
