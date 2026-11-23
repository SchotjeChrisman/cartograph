//! The one error type of the graph crate.

use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// No graph file exists yet at this path.
    NotIndexed(PathBuf),
    /// The graph file was written by a different schema version.
    SchemaVersion {
        found: i64,
        expected: i64,
    },
    /// Another process is the indexer.
    IndexerLockHeld,
    /// A fact handed to the loader breaks a rule of the graph model.
    InvalidFact(String),
    /// A job was finished by a process that does not hold its claim.
    JobNotOwned {
        kind: String,
        key: String,
    },
    /// The database holds a value this version does not understand.
    CorruptValue(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sqlite(error) => write!(formatter, "graph database error: {error}"),
            Error::Io(error) => write!(formatter, "graph io error: {error}"),
            Error::NotIndexed(path) => {
                write!(
                    formatter,
                    "no graph at {}; run `cartograph index` first",
                    path.display()
                )
            }
            Error::SchemaVersion { found, expected } => write!(
                formatter,
                "graph schema version {found} does not match {expected}; re-index to rebuild it"
            ),
            Error::IndexerLockHeld => write!(formatter, "indexing in progress in another process"),
            Error::InvalidFact(message) => write!(formatter, "invalid fact: {message}"),
            Error::JobNotOwned { kind, key } => {
                write!(formatter, "job {kind} {key} is not claimed by this owner")
            }
            Error::CorruptValue(message) => write!(formatter, "corrupt graph value: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Sqlite(error) => Some(error),
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Error::Sqlite(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}
