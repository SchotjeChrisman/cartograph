//! The staleness check every query runs first: a few changed files are re-indexed inline, many
//! are only reported. Which process re-indexes is decided by a claim on the `jobs` table, so
//! thirty agents noticing the same change cause one index run.
//!
//! "Changed" means modified at or after the graph's `indexed_at` second, new, or gone. That
//! costs one directory walk and `stat` calls, no hashing; the indexer itself hashes content, so
//! a touched but unchanged file costs one cheap no-op run.
//!
//! ponytail: reads the whole file table per query and misses an edit made in the same second as
//! the previous index finished; add per-file mtimes to the files table if either shows up in
//! a benchmark. Edits to `Cargo.toml` and `compile_commands.json` are not noticed either.

use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use graph::{Claim, ContentHash, GraphDir, JobOutcome, Query, Reader, UnixSeconds, Writer};
use ignore::WalkBuilder;
use index::{IndexOptions, index_directory};
use query::Staleness;

/// More changed files than this are not re-indexed inline.
pub const MAX_INLINE_FILES: usize = 100;
const JOB_KIND: &str = "auto-index";
/// Longer than any inline run, so a crashed claimant frees the work soon.
const CLAIM_LEASE_SECONDS: i64 = 120;
const GIT_ENTRY: &str = ".git";
const GRAPH_DIR_NAME: &str = ".cartograph";

#[derive(Debug)]
pub enum Error {
    Graph(graph::Error),
    Index(index::Error),
    Walk(ignore::Error),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(error) => write!(formatter, "{error}"),
            Error::Index(error) => write!(formatter, "re-index failed: {error}"),
            Error::Walk(error) => write!(formatter, "checking for changed files failed: {error}"),
            Error::Io(error) => write!(formatter, "checking for changed files failed: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<graph::Error> for Error {
    fn from(error: graph::Error) -> Self {
        Error::Graph(error)
    }
}

impl From<index::Error> for Error {
    fn from(error: index::Error) -> Self {
        Error::Index(error)
    }
}

impl From<ignore::Error> for Error {
    fn from(error: ignore::Error) -> Self {
        Error::Walk(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}

/// A tracked file as found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFile {
    pub path: String,
    pub modified: Duration,
}

/// One file the graph is out of date for. `modified` is `None` for a file that is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub modified: Option<Duration>,
}

/// Compares the graph's file list and age with what is on disk. Pure; the walk is elsewhere.
pub fn find_changes(
    indexed_at: UnixSeconds,
    graph_paths: &HashSet<String>,
    disk: &[DiskFile],
) -> Vec<Change> {
    let on_disk: HashSet<&str> = disk.iter().map(|file| file.path.as_str()).collect();
    let modified = disk
        .iter()
        .filter(|file| file.modified.as_secs() as i64 >= indexed_at)
        .map(|file| Change {
            path: file.path.clone(),
            modified: Some(file.modified),
        });
    let gone = graph_paths
        .iter()
        .filter(|path| !is_nested_graph(path) && !on_disk.contains(path.as_str()))
        .map(|path| Change {
            path: path.clone(),
            modified: None,
        });
    let mut changes: Vec<Change> = modified.chain(gone).collect();
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

/// Nested graphs are recorded as files in the graph but are not walked here.
fn is_nested_graph(path: &str) -> bool {
    path.rsplit('/').next() == Some(GRAPH_DIR_NAME)
}

/// Names the exact work, so a finished claim is never taken again but a new edit always is.
fn work_key(changes: &[Change]) -> String {
    let mut text = String::new();
    for change in changes {
        let stamp = change
            .modified
            .map_or_else(|| "gone".to_string(), |time| time.as_nanos().to_string());
        text.push_str(&format!("{}\t{stamp}\n", change.path));
    }
    ContentHash::of_bytes(text.as_bytes()).to_string()
}

/// Runs the staleness check for the graph at `root`.
pub fn check(root: &Path) -> Result<Staleness, Error> {
    let dir = GraphDir::of_root(root);
    let reader = Reader::open(&dir)?;
    let Some(indexed_at) = reader.freshness()?.indexed_at else {
        return Ok(Staleness::Fresh);
    };
    let graph_paths: HashSet<String> = reader.files()?.into_iter().map(|row| row.path).collect();
    let disk = scan_tracked_files(root, &graph_paths)?;
    let changes = find_changes(indexed_at, &graph_paths, &disk);
    if changes.is_empty() {
        return Ok(Staleness::Fresh);
    }
    if changes.len() > MAX_INLINE_FILES {
        return Ok(Staleness::Stale {
            changed_files: changes.len(),
        });
    }
    reindex_if_claimed(root, &dir, &changes)
}

fn reindex_if_claimed(root: &Path, dir: &GraphDir, changes: &[Change]) -> Result<Staleness, Error> {
    let key = work_key(changes);
    let owner = format!("cli-{}", std::process::id());
    let mut writer = Writer::open(dir)?;
    let claim = writer.claim_job(JOB_KIND, &key, &owner, CLAIM_LEASE_SECONDS, graph::now())?;
    match claim {
        Claim::AlreadyDone => Ok(Staleness::Fresh),
        Claim::HeldBy { .. } => Ok(stale(changes)),
        Claim::Acquired => {
            let outcome = index_directory(root, &IndexOptions::default());
            finish(&mut writer, &key, &owner, &outcome)?;
            match outcome {
                Ok(_) => {
                    crate::stale_components::mark(dir, &mut writer)?;
                    Ok(Staleness::Reindexed {
                        files: changes.len(),
                    })
                }
                Err(index::Error::Graph(graph::Error::IndexerLockHeld)) => Ok(stale(changes)),
                Err(error) => Err(error.into()),
            }
        }
    }
}

fn stale(changes: &[Change]) -> Staleness {
    Staleness::Stale {
        changed_files: changes.len(),
    }
}

fn finish(
    writer: &mut Writer,
    key: &str,
    owner: &str,
    outcome: &Result<index::IndexReport, index::Error>,
) -> Result<(), Error> {
    let job_outcome = match outcome {
        Ok(_) => JobOutcome::Done,
        Err(error) => JobOutcome::Failed(error.to_string()),
    };
    Ok(writer.finish_job(JOB_KIND, key, owner, &job_outcome, graph::now())?)
}

/// Walks like the indexer does and reports the files it would record: those already in the graph
/// and source files in a recognised language.
fn scan_tracked_files(root: &Path, graph_paths: &HashSet<String>) -> Result<Vec<DiskFile>, Error> {
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false)
        .require_git(false)
        .git_global(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name();
            name != GIT_ENTRY && name != GRAPH_DIR_NAME
        });
    let mut files = Vec::new();
    for entry in walk.build() {
        let entry = entry?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(path) = relative_path(root, entry.path()) else {
            continue;
        };
        if !graph_paths.contains(&path) && !is_recognised_source(&path) {
            continue;
        }
        let modified = entry
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        files.push(DiskFile { path, modified });
    }
    Ok(files)
}

fn is_recognised_source(path: &str) -> bool {
    languages::module_for_path(path).is_some() || languages::unsupported_language(path).is_some()
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect();
    Some(parts?.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn file(path: &str, seconds: u64) -> DiskFile {
        DiskFile {
            path: path.to_string(),
            modified: Duration::from_secs(seconds),
        }
    }

    #[test]
    fn finds_modified_new_and_gone_files_only() {
        let known = paths(&["old.rs", "touched.rs", "gone.rs", "sub/.cartograph"]);
        let disk = [
            file("old.rs", 50),
            file("touched.rs", 100),
            file("new.rs", 120),
        ];
        let changes = find_changes(100, &known, &disk);
        let summary: Vec<(&str, bool)> = changes
            .iter()
            .map(|change| (change.path.as_str(), change.modified.is_some()))
            .collect();
        assert_eq!(
            summary,
            [("gone.rs", false), ("new.rs", true), ("touched.rs", true)]
        );
    }

    #[test]
    fn work_key_changes_with_the_edit_but_not_with_the_observer() {
        let change = |nanos| Change {
            path: "a.rs".to_string(),
            modified: Some(Duration::from_nanos(nanos)),
        };
        assert_eq!(work_key(&[change(5)]), work_key(&[change(5)]));
        assert_ne!(work_key(&[change(5)]), work_key(&[change(6)]));
    }
}
