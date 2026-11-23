//! Shared SQL fragments and row decoding for reads.

use rusqlite::Row;
use rusqlite::types::Type;

use crate::components::ComponentId;
use crate::csr::Direction;
use crate::facts::FileTags;
use crate::id::SymbolId;
use crate::kinds::{EdgeKind, NodeKind};
use crate::query::NodeInfo;

/// Columns of a node joined to its file as `n` and `f`; decoded by [`node_from_row`].
pub const NODE_COLUMNS: &str =
    "n.id, n.kind, n.name, n.qualified_name, f.path, n.line, f.component_id, n.degree, f.tags";
pub const NODE_COLUMN_COUNT: usize = 9;

/// Reads a node from [`NODE_COLUMNS`] starting at column `first`.
pub fn node_from_row(row: &Row, first: usize) -> rusqlite::Result<NodeInfo> {
    let id: i64 = row.get(first)?;
    let component: Option<i64> = row.get(first + 6)?;
    let tags: Option<i64> = row.get(first + 8)?;
    Ok(NodeInfo {
        id: symbol_id(id, first)?,
        kind: decode(
            row.get(first + 1)?,
            first + 1,
            NodeKind::from_code,
            "node kind",
        )?,
        name: row.get(first + 2)?,
        qualified_name: row.get(first + 3)?,
        file: row.get(first + 4)?,
        line: row.get(first + 5)?,
        component: component.map(ComponentId),
        degree: row.get(first + 7)?,
        tags: FileTags::from_bits(tags.unwrap_or(0)),
    })
}

/// A stored node id; negative values cannot come from [`SymbolId`], so they mean corruption.
pub fn symbol_id(value: i64, column: usize) -> rusqlite::Result<SymbolId> {
    SymbolId::from_i64(value).ok_or_else(|| conversion_error(column, "negative node id"))
}

/// Turns a stored code into an enum, failing the row loudly when the code is unknown.
pub fn decode<T>(
    code: i64,
    column: usize,
    from_code: fn(i64) -> Option<T>,
    what: &str,
) -> rusqlite::Result<T> {
    from_code(code).ok_or_else(|| conversion_error(column, &format!("unknown {what} code {code}")))
}

fn conversion_error(column: usize, message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, message.into())
}

/// ` AND <column> IN (...)` for a non-empty kind list, empty otherwise. The codes are integers
/// from our own enum, so formatting them in is safe.
pub fn kind_filter(column: &str, kinds: &[EdgeKind]) -> String {
    if kinds.is_empty() {
        return String::new();
    }
    let codes: Vec<String> = kinds.iter().map(|kind| kind.code().to_string()).collect();
    format!(" AND {column} IN ({})", codes.join(","))
}

/// The edge column holding the node a query starts from, and the one holding the other end.
pub fn edge_columns(direction: Direction) -> (&'static str, &'static str) {
    match direction {
        Direction::In => ("dst", "src"),
        _ => ("src", "dst"),
    }
}

/// A row limit as SQLite's integer type, saturating for huge values.
pub fn sql_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// A non-negative count column such as `count(*)`.
pub fn count_column(row: &Row, column: usize) -> rusqlite::Result<u64> {
    let count: i64 = row.get(column)?;
    u64::try_from(count).map_err(|_| conversion_error(column, "negative count"))
}
