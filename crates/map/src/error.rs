//! The one error type of the map crate.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The listening socket could not be opened.
    Bind(Box<dyn std::error::Error + Send + Sync>),
    Graph(graph::Error),
    Describe(describe::Error),
    Overrides(describe::OverridesError),
    /// The request itself is wrong: a missing or malformed parameter.
    BadRequest(String),
    /// The request names something the graph does not have.
    NotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Bind(error) => write!(formatter, "cannot start the map server: {error}"),
            Error::Graph(error) => write!(formatter, "graph: {error}"),
            Error::Describe(error) => write!(formatter, "describe: {error}"),
            Error::Overrides(error) => write!(formatter, "{error}"),
            Error::BadRequest(message) => write!(formatter, "bad request: {message}"),
            Error::NotFound(what) => write!(formatter, "not found: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Graph(error) => Some(error),
            Error::Describe(error) => Some(error),
            Error::Overrides(error) => Some(error),
            Error::Bind(error) => Some(error.as_ref()),
            Error::BadRequest(_) | Error::NotFound(_) => None,
        }
    }
}

impl From<graph::Error> for Error {
    fn from(error: graph::Error) -> Self {
        Error::Graph(error)
    }
}

impl From<describe::Error> for Error {
    fn from(error: describe::Error) -> Self {
        Error::Describe(error)
    }
}

impl From<describe::OverridesError> for Error {
    fn from(error: describe::OverridesError) -> Self {
        Error::Overrides(error)
    }
}
