//! The one error type of the describe crate.

use std::fmt;

use crate::backend::BackendError;
use crate::output::OutputError;
use crate::overrides::OverridesError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Graph(graph::Error),
    Backend(BackendError),
    /// The model answered, but not with a valid description.
    Output(OutputError),
    Overrides(OverridesError),
    /// A job for exactly this work is marked done, yet no current description exists.
    DoneWithoutDescription {
        component: i64,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(error) => write!(formatter, "{error}"),
            Error::Backend(error) => write!(formatter, "{error}"),
            Error::Output(error) => write!(formatter, "{error}"),
            Error::Overrides(error) => write!(formatter, "{error}"),
            Error::DoneWithoutDescription { component } => write!(
                formatter,
                "a finished describe job exists for component {component} but its description is missing"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Graph(error) => Some(error),
            Error::Backend(error) => Some(error),
            Error::Output(error) => Some(error),
            Error::Overrides(error) => Some(error),
            Error::DoneWithoutDescription { .. } => None,
        }
    }
}

impl From<graph::Error> for Error {
    fn from(error: graph::Error) -> Self {
        Error::Graph(error)
    }
}

impl From<BackendError> for Error {
    fn from(error: BackendError) -> Self {
        Error::Backend(error)
    }
}

impl From<OutputError> for Error {
    fn from(error: OutputError) -> Self {
        Error::Output(error)
    }
}

impl From<OverridesError> for Error {
    fn from(error: OverridesError) -> Self {
        Error::Overrides(error)
    }
}
