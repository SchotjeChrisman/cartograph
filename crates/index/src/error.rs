use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Graph(graph::Error),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Walk(ignore::Error),
    /// `.cartograph/config.toml` is not valid.
    Config {
        path: PathBuf,
        source: toml::de::Error,
    },
    /// A cached extraction cannot be decoded.
    CacheEntry {
        path: String,
        source: serde_json::Error,
    },
    /// The root holds only git worktrees; each is its own project.
    OnlyWorktrees {
        root: PathBuf,
        worktrees: Vec<String>,
    },
    /// The cache lost an extraction this run had just stored or relied on.
    CacheEntryMissing(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(error) => write!(formatter, "{error}"),
            Error::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Error::Walk(error) => write!(formatter, "walking the tree failed: {error}"),
            Error::Config { path, source } => write!(formatter, "{}: {source}", path.display()),
            Error::CacheEntry { path, source } => write!(
                formatter,
                "cached extraction of {path} is unreadable: {source}; delete the extraction cache to force a full re-index"
            ),
            Error::OnlyWorktrees { root, worktrees } => write!(
                formatter,
                "{} holds git worktrees and no source of its own; run cartograph inside one of: {}",
                root.display(),
                worktrees.join(", ")
            ),
            Error::CacheEntryMissing(path) => write!(
                formatter,
                "the extraction cache has no entry for {path}; run the index again"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<graph::Error> for Error {
    fn from(error: graph::Error) -> Self {
        Error::Graph(error)
    }
}

impl From<ignore::Error> for Error {
    fn from(error: ignore::Error) -> Self {
        Error::Walk(error)
    }
}

pub(crate) fn io_error(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}
