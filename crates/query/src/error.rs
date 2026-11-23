//! Failures of a query. A name that matches nothing is an answer, not an error.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Graph(graph::Error),
    /// The staleness hook failed, for example because inline re-indexing could not finish.
    Staleness(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(error) => write!(formatter, "{error}"),
            Error::Staleness(error) => write!(formatter, "staleness check failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Graph(error) => Some(error),
            Error::Staleness(error) => Some(error.as_ref()),
        }
    }
}

impl From<graph::Error> for Error {
    fn from(error: graph::Error) -> Self {
        Error::Graph(error)
    }
}
