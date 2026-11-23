//! Freshness: the hook index plugs in, and the notes that tell a reader how far to trust an answer.

use graph::{Freshness, UnixSeconds};

use crate::error::{Error, Result};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// What the staleness check found, and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    Fresh,
    /// A few files had changed and were re-indexed before answering.
    Reindexed {
        files: usize,
    },
    /// Too many files changed to re-index inline.
    Stale {
        changed_files: usize,
    },
}

/// Run before every query. The index crate implements it (compare hashes, re-index a few files
/// inline); query only defines the seam, so there is no dependency cycle. Any
/// `Fn() -> Result<Staleness, E>` closure works.
pub trait StalenessCheck {
    fn check(&self) -> Result<Staleness>;
}

impl<F, E> StalenessCheck for F
where
    F: Fn() -> std::result::Result<Staleness, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    fn check(&self) -> Result<Staleness> {
        self().map_err(|error| Error::Staleness(Box::new(error)))
    }
}

/// The check used when none is plugged in.
pub struct NoCheck;

impl StalenessCheck for NoCheck {
    fn check(&self) -> Result<Staleness> {
        Ok(Staleness::Fresh)
    }
}

/// Lines printed above every answer when the graph is not plainly current.
pub fn notes(staleness: Staleness, freshness: &Freshness, now: UnixSeconds) -> Vec<String> {
    let mut notes = Vec::new();
    match freshness.indexed_at {
        None => notes.push("warning: the graph is empty; run `cartograph index`".to_string()),
        Some(indexed_at) => {
            if let Staleness::Stale { changed_files } = staleness {
                notes.push(format!(
                    "warning: {changed_files} files changed since the graph was indexed {} ago; \
                     results may be out of date; run `cartograph index`",
                    age(now - indexed_at)
                ));
            }
        }
    }
    if let Staleness::Reindexed { files } = staleness {
        notes.push(format!(
            "note: re-indexed {files} changed files before answering"
        ));
    }
    if freshness.indexing_in_progress {
        notes.push(
            "note: indexing in progress in another process; showing the last committed snapshot"
                .to_string(),
        );
    }
    notes
}

fn age(seconds: i64) -> String {
    match seconds.max(0) {
        s if s >= DAY => format!("{}d", s / DAY),
        s if s >= HOUR => format!("{}h", s / HOUR),
        s if s >= MINUTE => format!("{}m", s / MINUTE),
        s => format!("{s}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freshness(indexed_at: Option<i64>, indexing_in_progress: bool) -> Freshness {
        Freshness {
            indexed_at,
            indexing_in_progress,
        }
    }

    #[test]
    fn current_graph_has_no_notes() {
        assert!(notes(Staleness::Fresh, &freshness(Some(0), false), 10).is_empty());
    }

    #[test]
    fn stale_graph_warns_with_age() {
        let found = notes(
            Staleness::Stale { changed_files: 40 },
            &freshness(Some(0), false),
            3 * HOUR + 5,
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("40 files changed") && found[0].contains("3h ago"));
    }

    #[test]
    fn indexing_and_empty_graph_are_reported() {
        let found = notes(Staleness::Fresh, &freshness(None, true), 0);
        assert!(found[0].contains("empty"));
        assert!(found[1].contains("indexing in progress"));
    }

    #[test]
    fn closures_plug_in_as_checks() {
        let check = || Ok::<_, std::io::Error>(Staleness::Reindexed { files: 2 });
        assert_eq!(check.check().unwrap(), Staleness::Reindexed { files: 2 });
    }
}
