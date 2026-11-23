//! Opening the graph file: one `Writer` for all writes, any number of `Reader`s.

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::{Error, Result};
use crate::layout::GraphDir;
use crate::schema::{self, CONNECTION_PRAGMAS, SCHEMA_VERSION, WRITER_PRAGMAS};

/// Seconds since the Unix epoch. Callers pass it in, so time stays at the edges.
pub type UnixSeconds = i64;

/// The current time as [`UnixSeconds`].
pub fn now() -> UnixSeconds {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// A read-write connection. Bulk loading additionally needs the [`crate::IndexerLock`].
pub struct Writer {
    pub(crate) connection: Connection,
}

impl Writer {
    /// Opens the graph for writing, creating the directory, file and schema when missing.
    pub fn open(dir: &GraphDir) -> Result<Self> {
        dir.create()?;
        let mut connection = Connection::open(dir.database_path())?;
        connection.execute_batch(CONNECTION_PRAGMAS)?;
        connection.execute_batch(WRITER_PRAGMAS)?;
        create_schema_if_missing(&mut connection)?;
        Ok(Writer { connection })
    }
}

fn create_schema_if_missing(connection: &mut Connection) -> Result<()> {
    // A plain read first: an existing graph must not queue behind the indexer's write lock.
    let existing: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if existing == SCHEMA_VERSION {
        return Ok(());
    }
    // IMMEDIATE so two processes creating the same graph cannot both build the tables.
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match found {
        0 => {
            transaction.execute_batch(&schema::create_statements())?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        SCHEMA_VERSION => {}
        _ => {
            return Err(Error::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            });
        }
    }
    Ok(transaction.commit()?)
}

/// A read-only connection. In WAL mode it reads the last committed snapshot and never waits for
/// the writer.
pub struct Reader {
    pub(crate) connection: Connection,
    pub(crate) dir: GraphDir,
}

impl Reader {
    /// Opens an existing graph; fails with [`Error::NotIndexed`] when there is none.
    pub fn open(dir: &GraphDir) -> Result<Self> {
        let path = dir.database_path();
        if !path.exists() {
            return Err(Error::NotIndexed(path));
        }
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch(CONNECTION_PRAGMAS)?;
        let found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if found != SCHEMA_VERSION {
            return Err(Error::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Reader {
            connection,
            dir: dir.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_are_not_blocked_by_an_open_write_transaction() {
        let scratch = std::env::temp_dir().join(format!("graph-open-txn-{}", std::process::id()));
        let dir = GraphDir::at(scratch.clone());
        let mut writer = Writer::open(&dir).unwrap();
        let reader = Reader::open(&dir).unwrap();

        let transaction = writer
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction
            .execute(
                "INSERT INTO meta (key, value) VALUES ('k', 'uncommitted')",
                [],
            )
            .unwrap();
        let during: i64 = reader
            .connection
            .query_row("SELECT count(*) FROM meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(during, 0, "reader sees the last committed snapshot");

        transaction.commit().unwrap();
        let after: i64 = reader
            .connection
            .query_row("SELECT count(*) FROM meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, 1);
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
