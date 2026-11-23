//! The indexer's private cache of what it extracted from each file, kept in a database of its own
//! so the graph file stays small for the many processes that only read it, and so extractions
//! can be written while the graph loads. The blobs are opaque here: the indexer encodes and
//! decodes them.
//!
//! The graph database stays the source of truth. An entry is used only when its hash equals the
//! file's current hash, and the whole cache is thrown away when the settings it was made under
//! change or when a run did not finish (the cache may then be ahead of the graph).

use std::collections::HashMap;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::error::Result;
use crate::id::ContentHash;
use crate::layout::GraphDir;

/// Entries per transaction when storing.
const STORE_BATCH: usize = 500;
/// Readers map the file instead of copying pages out of it; many reading threads then share one
/// copy of each page.
const READER_MMAP_BYTES: u64 = 4 << 30;
const FINGERPRINT_KEY: &str = "fingerprint";
const UNFINISHED_RUN_KEY: &str = "unfinished_run";

/// What the cache holds for one file. In the table `shared` comes before `full`: a row longer than
/// a page spills into overflow pages, and a column is read by walking the row only as far as it
/// reaches, so the small blob read for every file does not drag the large one along.
pub struct CachedExtraction {
    pub path: String,
    /// Hash of the file bytes the blobs were made from.
    pub hash: ContentHash,
    /// Everything extracted from the file.
    pub full: Vec<u8>,
    /// The small part other files can see; read for every file on most runs.
    pub shared: Vec<u8>,
}

pub struct ExtractionCache {
    connection: Connection,
}

impl ExtractionCache {
    /// Opens the cache for writing, emptying it when `fingerprint` differs from the one it was
    /// filled under or when the last run that used it never finished.
    pub fn open(dir: &GraphDir, fingerprint: &str) -> Result<Self> {
        dir.create()?;
        let connection = Connection::open(dir.extraction_cache_path())?;
        // The cache can always be rebuilt, so durability is traded for write speed.
        connection.execute_batch(
            "PRAGMA page_size = 16384;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = OFF;
             PRAGMA busy_timeout = 10000;
             CREATE TABLE IF NOT EXISTS entries (
                 path TEXT PRIMARY KEY,
                 hash BLOB NOT NULL,
                 shared BLOB NOT NULL,
                 full BLOB NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let cache = ExtractionCache { connection };
        let usable = cache.meta(FINGERPRINT_KEY)?.as_deref() == Some(fingerprint)
            && cache.meta(UNFINISHED_RUN_KEY)?.is_none();
        if !usable {
            cache.connection.execute("DELETE FROM entries", [])?;
            cache.connection.execute("DELETE FROM meta", [])?;
            cache.set_meta(FINGERPRINT_KEY, fingerprint)?;
        }
        Ok(cache)
    }

    /// A second connection to the same cache, for one reading thread.
    pub fn open_reader(dir: &GraphDir) -> Result<Self> {
        let connection = Connection::open_with_flags(
            dir.extraction_cache_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch(&format!(
            "PRAGMA busy_timeout = 10000; PRAGMA mmap_size = {READER_MMAP_BYTES};"
        ))?;
        Ok(ExtractionCache { connection })
    }

    /// Marks that the graph may lag behind the cache until [`Self::mark_run_finished`].
    pub fn mark_run_started(&self) -> Result<()> {
        self.set_meta(UNFINISHED_RUN_KEY, "1")
    }

    pub fn mark_run_finished(&self) -> Result<()> {
        self.connection
            .execute("DELETE FROM meta WHERE key = ?1", [UNFINISHED_RUN_KEY])?;
        Ok(())
    }

    /// The hash each cached file was extracted from.
    pub fn hashes(&self) -> Result<HashMap<String, ContentHash>> {
        let mut statement = self.connection.prepare("SELECT path, hash FROM entries")?;
        let rows = statement.query_map([], |row| {
            let hash: Vec<u8> = row.get(1)?;
            Ok((row.get::<_, String>(0)?, hash))
        })?;
        let mut hashes = HashMap::new();
        for row in rows {
            let (path, hash) = row?;
            if let Some(hash) = ContentHash::from_slice(&hash) {
                hashes.insert(path, hash);
            }
        }
        Ok(hashes)
    }

    /// Stores entries, replacing any for the same paths, `STORE_BATCH` per transaction.
    pub fn store(&mut self, entries: impl IntoIterator<Item = CachedExtraction>) -> Result<()> {
        let mut entries = entries.into_iter().peekable();
        while entries.peek().is_some() {
            let transaction = self.connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(
                    "INSERT OR REPLACE INTO entries (path, hash, shared, full)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                for entry in entries.by_ref().take(STORE_BATCH) {
                    statement.execute(params![
                        entry.path,
                        entry.hash.as_bytes().as_slice(),
                        entry.shared,
                        entry.full
                    ])?;
                }
            }
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn remove(&mut self, paths: &[&str]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for path in paths {
            transaction.execute("DELETE FROM entries WHERE path = ?1", [path])?;
        }
        Ok(transaction.commit()?)
    }

    pub fn full(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.blob("full", path)
    }

    pub fn shared(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.blob("shared", path)
    }

    fn blob(&self, column: &str, path: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .connection
            .prepare_cached(&format!("SELECT {column} FROM entries WHERE path = ?1"))?
            .query_row([path], |row| row.get(0))
            .optional()?)
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}
