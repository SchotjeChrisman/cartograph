//! Batched bulk loading. Parsers run in parallel and feed one writer; each batch is one
//! transaction, so readers always see a consistent snapshot and a crash loses at most one batch.

use std::collections::HashSet;
use std::ops::AddAssign;

use rusqlite::{Transaction, TransactionBehavior, params};

use crate::error::{Error, Result};
use crate::facts::{EdgeFact, FileFacts, NodeFact};
use crate::lock::IndexerLock;
use crate::names::split_words;
use crate::schema::bulk_load_indexes;
use crate::store::{UnixSeconds, Writer};

/// Files per transaction when the caller has no better number.
pub const DEFAULT_BATCH_FILES: usize = 500;

const INDEXED_AT_KEY: &str = "indexed_at";

/// What a load wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadStats {
    pub files: usize,
    pub nodes: usize,
    pub edges: usize,
}

/// Size and modification time to record for a file whose content did not change.
pub struct FileStamp<'a> {
    pub path: &'a str,
    pub size_bytes: u64,
    pub mtime_ns: Option<i64>,
}

impl AddAssign for LoadStats {
    fn add_assign(&mut self, other: LoadStats) {
        self.files += other.files;
        self.nodes += other.nodes;
        self.edges += other.edges;
    }
}

/// How a load treats the tables.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadMode {
    /// The graph already holds files: rows are replaced one file at a time and derived data
    /// (degrees, word index) is kept current inside each batch.
    Incremental,
    /// The graph holds no files: everything goes in one transaction with the secondary indexes
    /// and the word index left out, and those are built once at the end from the finished rows.
    /// Random symbol ids make row-by-row index maintenance the dominant cost of a first load.
    Bulk,
}

/// Bookkeeping collected while a batch is applied and settled just before it commits.
struct BatchEffects {
    mode: LoadMode,
    /// Nodes whose edge count may have changed.
    touched_nodes: HashSet<i64>,
    /// Components that lost or changed a file, so they and their ancestors are now stale.
    changed_components: HashSet<i64>,
}

impl BatchEffects {
    fn new(mode: LoadMode) -> Self {
        BatchEffects {
            mode,
            touched_nodes: HashSet::new(),
            changed_components: HashSet::new(),
        }
    }

    /// A bulk load recomputes every degree at the end, so it keeps no list.
    fn touch_node(&mut self, id: i64) {
        if self.mode == LoadMode::Incremental {
            self.touched_nodes.insert(id);
        }
    }
}

impl Writer {
    /// Loads facts. A file whose path is already in the graph has its old nodes and edges
    /// replaced, `batch_files` files per transaction. Into a graph with no files the load is one
    /// transaction, so readers see the graph appear whole. Only the indexer may load, hence the lock.
    pub fn load(
        &mut self,
        _lock: &IndexerLock,
        facts: impl IntoIterator<Item = FileFacts>,
        batch_files: usize,
        now: UnixSeconds,
    ) -> Result<LoadStats> {
        let mut facts = facts.into_iter().peekable();
        if facts.peek().is_none() {
            return Ok(LoadStats::default());
        }
        if self.holds_no_files()? {
            return self.load_in_bulk(facts, now);
        }
        let mut total = LoadStats::default();
        let mut batch = Vec::with_capacity(batch_files);
        for file_facts in facts {
            batch.push(file_facts);
            if batch.len() >= batch_files {
                total += self.commit_batch(&batch, now)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            total += self.commit_batch(&batch, now)?;
        }
        Ok(total)
    }

    /// Removes files that no longer exist, with their nodes and edges. Returns how many were known.
    pub fn remove_files(
        &mut self,
        _lock: &IndexerLock,
        paths: &[String],
        now: UnixSeconds,
    ) -> Result<usize> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut effects = BatchEffects::new(LoadMode::Incremental);
        let mut removed = 0;
        for path in paths {
            if let Some((file_id, component)) = find_file(&transaction, path)? {
                clear_file_contents(&transaction, file_id, component, &mut effects)?;
                transaction.execute("DELETE FROM files WHERE id = ?1", [file_id])?;
                removed += 1;
            }
        }
        settle(&transaction, effects, now)?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Records new sizes and modification times for files that were read again and found
    /// unchanged, so later runs can skip reading them.
    pub fn refresh_stamps(&mut self, _lock: &IndexerLock, stamps: &[FileStamp]) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for stamp in stamps {
            transaction
                .prepare_cached("UPDATE files SET size_bytes = ?1, mtime_ns = ?2 WHERE path = ?3")?
                .execute(params![stamp.size_bytes as i64, stamp.mtime_ns, stamp.path])?;
        }
        Ok(transaction.commit()?)
    }

    fn commit_batch(&mut self, batch: &[FileFacts], now: UnixSeconds) -> Result<LoadStats> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut effects = BatchEffects::new(LoadMode::Incremental);
        let mut stats = LoadStats::default();
        for file_facts in batch {
            stats += apply_file(&transaction, file_facts, now, &mut effects)?;
        }
        settle(&transaction, effects, now)?;
        transaction.commit()?;
        Ok(stats)
    }

    fn holds_no_files(&self) -> Result<bool> {
        Ok(!self
            .connection
            .query_row("SELECT EXISTS (SELECT 1 FROM files)", [], |row| {
                row.get::<_, bool>(0)
            })?)
    }

    fn load_in_bulk(
        &mut self,
        facts: impl Iterator<Item = FileFacts>,
        now: UnixSeconds,
    ) -> Result<LoadStats> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for index in bulk_load_indexes() {
            transaction.execute_batch(&format!("DROP INDEX {}", index.name))?;
        }
        let mut effects = BatchEffects::new(LoadMode::Bulk);
        let mut stats = LoadStats::default();
        for file_facts in facts {
            stats += apply_file(&transaction, &file_facts, now, &mut effects)?;
        }
        for index in bulk_load_indexes() {
            transaction.execute_batch(&index.create_statement())?;
        }
        refresh_all_degrees(&transaction)?;
        drop_orphaned_globals(&transaction)?;
        fill_node_words(&transaction)?;
        record_indexed_at(&transaction, now)?;
        transaction.commit()?;
        Ok(stats)
    }
}

fn find_file(transaction: &Transaction, path: &str) -> Result<Option<(i64, Option<i64>)>> {
    let mut statement =
        transaction.prepare_cached("SELECT id, component_id FROM files WHERE path = ?1")?;
    let mut rows = statement.query([path])?;
    match rows.next()? {
        Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
        None => Ok(None),
    }
}

fn apply_file(
    transaction: &Transaction,
    file_facts: &FileFacts,
    now: UnixSeconds,
    effects: &mut BatchEffects,
) -> Result<LoadStats> {
    let path = &file_facts.file.path;
    for edge in &file_facts.edges {
        edge.validate()?;
    }
    let file_id = upsert_file(transaction, file_facts, now, effects)?;
    for node in &file_facts.nodes {
        insert_node(transaction, node, path, file_id, effects)?;
    }
    for edge in &file_facts.edges {
        insert_edge(transaction, edge, file_id, effects)?;
    }
    Ok(LoadStats {
        files: 1,
        nodes: file_facts.nodes.len(),
        edges: file_facts.edges.len(),
    })
}

/// Inserts or updates the file row, emptying the file's previous nodes and edges first.
fn upsert_file(
    transaction: &Transaction,
    file_facts: &FileFacts,
    now: UnixSeconds,
    effects: &mut BatchEffects,
) -> Result<i64> {
    let file = &file_facts.file;
    let hash = file
        .content_hash
        .as_ref()
        .map(|hash| hash.as_bytes().as_slice());
    let values = params![
        file.repo,
        file.language,
        hash,
        file.size_bytes as i64,
        file.mtime_ns,
        file.status.code(),
        file.status_detail,
        file.tags.to_bits(),
        now,
        file.path
    ];
    if let Some((file_id, component)) = find_file(transaction, &file.path)? {
        clear_file_contents(transaction, file_id, component, effects)?;
        transaction
            .prepare_cached(
                "UPDATE files SET repo = ?1, language = ?2, content_hash = ?3, size_bytes = ?4,
                 mtime_ns = ?5, status = ?6, status_detail = ?7, tags = ?8, indexed_at = ?9
                 WHERE path = ?10",
            )?
            .execute(values)?;
        return Ok(file_id);
    }
    transaction
        .prepare_cached(
            "INSERT INTO files (repo, language, content_hash, size_bytes, mtime_ns, status,
             status_detail, tags, indexed_at, path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?
        .execute(values)?;
    Ok(transaction.last_insert_rowid())
}

fn clear_file_contents(
    transaction: &Transaction,
    file_id: i64,
    component: Option<i64>,
    effects: &mut BatchEffects,
) -> Result<()> {
    let mut statement =
        transaction.prepare_cached("SELECT src, dst FROM edges WHERE file_id = ?1")?;
    let mut rows = statement.query([file_id])?;
    while let Some(row) = rows.next()? {
        effects.touch_node(row.get(0)?);
        effects.touch_node(row.get(1)?);
    }
    transaction.execute("DELETE FROM edges WHERE file_id = ?1", [file_id])?;
    transaction.execute(
        "DELETE FROM node_words WHERE rowid IN (SELECT id FROM nodes WHERE file_id = ?1)",
        [file_id],
    )?;
    transaction.execute("DELETE FROM nodes WHERE file_id = ?1", [file_id])?;
    effects.changed_components.extend(component);
    Ok(())
}

fn insert_node(
    transaction: &Transaction,
    node: &NodeFact,
    file_path: &str,
    file_id: i64,
    effects: &mut BatchEffects,
) -> Result<()> {
    let id = node.id_in(file_path).as_i64();
    let owner = (!node.kind.is_global()).then_some(file_id);
    let inserted = transaction
        .prepare_cached(
            "INSERT INTO nodes (id, kind, name, qualified_name, file_id, line, end_line, signature, doc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT (id) DO NOTHING",
        )?
        .execute(params![
            id,
            node.kind.code(),
            node.name,
            node.qualified_name,
            owner,
            node.line,
            node.end_line,
            node.signature,
            node.doc
        ])?;
    if inserted == 0 && owner.is_some() {
        return Err(Error::InvalidFact(format!(
            "duplicate symbol {} {:?} in {file_path}; overloads need distinct qualified names",
            node.kind, node.qualified_name
        )));
    }
    if inserted == 1 && effects.mode == LoadMode::Incremental {
        insert_node_words(transaction, id, &node.qualified_name)?;
    }
    if owner.is_none() {
        effects.touch_node(id);
    }
    Ok(())
}

fn insert_edge(
    transaction: &Transaction,
    edge: &EdgeFact,
    file_id: i64,
    effects: &mut BatchEffects,
) -> Result<()> {
    transaction
        .prepare_cached(
            "INSERT INTO edges (src, dst, kind, provenance, file_id, line, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            edge.src.as_i64(),
            edge.dst.as_i64(),
            edge.kind.code(),
            edge.provenance.code(),
            file_id,
            edge.line,
            edge.reason
        ])?;
    effects.touch_node(edge.src.as_i64());
    effects.touch_node(edge.dst.as_i64());
    Ok(())
}

/// Brings derived data in line with the batch: degrees, orphaned global nodes, stale components
/// and the graph's age.
fn settle(transaction: &Transaction, effects: BatchEffects, now: UnixSeconds) -> Result<()> {
    for id in &effects.touched_nodes {
        refresh_degree(transaction, *id)?;
        drop_if_orphaned_global(transaction, *id)?;
    }
    for component in &effects.changed_components {
        mark_component_and_ancestors_stale(transaction, *component)?;
    }
    record_indexed_at(transaction, now)
}

fn record_indexed_at(transaction: &Transaction, now: UnixSeconds) -> Result<()> {
    transaction.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![INDEXED_AT_KEY, now.to_string()],
    )?;
    Ok(())
}

fn insert_node_words(transaction: &Transaction, id: i64, qualified_name: &str) -> Result<()> {
    transaction
        .prepare_cached("INSERT INTO node_words (rowid, words) VALUES (?1, ?2)")?
        .execute(params![id, split_words(qualified_name).join(" ")])?;
    Ok(())
}

/// Fills the word index from the finished nodes table, in id order: the index builds far faster
/// from sorted rowids than from the random order nodes arrive in.
fn fill_node_words(transaction: &Transaction) -> Result<()> {
    let mut statement = transaction.prepare("SELECT id, qualified_name FROM nodes ORDER BY id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        insert_node_words(transaction, row.get(0)?, &row.get::<_, String>(1)?)?;
    }
    Ok(())
}

fn refresh_all_degrees(transaction: &Transaction) -> Result<()> {
    transaction.execute(
        "UPDATE nodes SET degree = (SELECT count(*) FROM edges WHERE src = nodes.id)
                                  + (SELECT count(*) FROM edges WHERE dst = nodes.id)",
        [],
    )?;
    Ok(())
}

fn drop_orphaned_globals(transaction: &Transaction) -> Result<()> {
    transaction.execute("DELETE FROM nodes WHERE file_id IS NULL AND degree = 0", [])?;
    Ok(())
}

fn refresh_degree(transaction: &Transaction, id: i64) -> Result<()> {
    transaction
        .prepare_cached(
            "UPDATE nodes SET degree = (SELECT count(*) FROM edges WHERE src = ?1)
                                     + (SELECT count(*) FROM edges WHERE dst = ?1)
             WHERE id = ?1",
        )?
        .execute([id])?;
    Ok(())
}

fn drop_if_orphaned_global(transaction: &Transaction, id: i64) -> Result<()> {
    let orphaned = "SELECT id FROM nodes WHERE id = ?1 AND file_id IS NULL AND degree = 0";
    transaction
        .prepare_cached(&format!(
            "DELETE FROM node_words WHERE rowid IN ({orphaned})"
        ))?
        .execute([id])?;
    transaction
        .prepare_cached(&format!("DELETE FROM nodes WHERE id IN ({orphaned})"))?
        .execute([id])?;
    Ok(())
}

fn mark_component_and_ancestors_stale(transaction: &Transaction, component: i64) -> Result<()> {
    transaction
        .prepare_cached(
            "WITH RECURSIVE chain(id) AS (
                 SELECT ?1
                 UNION
                 SELECT components.parent_id FROM components JOIN chain ON components.id = chain.id
                 WHERE components.parent_id IS NOT NULL
             )
             UPDATE components SET stale = 1 WHERE id IN (SELECT id FROM chain)",
        )?
        .execute([component])?;
    Ok(())
}
