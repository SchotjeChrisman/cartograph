//! Job claims: many processes ask for the same work, one gets it. This stops thirty agents from
//! all re-indexing the same stale files or describing the same component.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::error::{Error, Result};
use crate::kinds::JobState;
use crate::store::{Reader, UnixSeconds, Writer};

/// The answer to a claim request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The caller owns the job and must finish it.
    Acquired,
    /// Another owner holds a claim that has not expired; serve the current snapshot and move on.
    HeldBy { owner: String },
    /// The work was already done; the key names the exact work, so it need not be repeated.
    AlreadyDone,
}

/// How a claimed job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    Done,
    Failed(String),
}

/// One row of the jobs table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: String,
    pub key: String,
    pub state: JobState,
    pub owner: String,
    pub claimed_at: UnixSeconds,
    pub finished_at: Option<UnixSeconds>,
    pub detail: Option<String>,
}

impl Writer {
    /// Claims the job `(kind, key)` for `owner`. The key must name the exact work, for example
    /// the component hash being described, because a finished key is never run again.
    /// A claim older than `lease_seconds` is treated as dead and can be taken over; a failed
    /// job can be claimed again.
    pub fn claim_job(
        &mut self,
        kind: &str,
        key: &str,
        owner: &str,
        lease_seconds: i64,
        now: UnixSeconds,
    ) -> Result<Claim> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(i64, String, UnixSeconds)> = transaction
            .query_row(
                "SELECT state, owner, claimed_at FROM jobs WHERE kind = ?1 AND key = ?2",
                [kind, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let claim = match existing {
            None => Claim::Acquired,
            Some((state, owner_now, claimed_at)) => match decode_state(state)? {
                JobState::Done => Claim::AlreadyDone,
                JobState::Claimed if claimed_at + lease_seconds > now => {
                    Claim::HeldBy { owner: owner_now }
                }
                JobState::Claimed | JobState::Failed => Claim::Acquired,
            },
        };
        if claim == Claim::Acquired {
            transaction.execute(
                "INSERT INTO jobs (kind, key, state, owner, claimed_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (kind, key) DO UPDATE SET state = excluded.state, owner = excluded.owner,
                 claimed_at = excluded.claimed_at, finished_at = NULL, detail = NULL",
                params![kind, key, JobState::Claimed.code(), owner, now],
            )?;
        }
        transaction.commit()?;
        Ok(claim)
    }

    /// Records how a job ended. Only the current claim holder may finish it.
    pub fn finish_job(
        &mut self,
        kind: &str,
        key: &str,
        owner: &str,
        outcome: &JobOutcome,
        now: UnixSeconds,
    ) -> Result<()> {
        let (state, detail) = match outcome {
            JobOutcome::Done => (JobState::Done, None),
            JobOutcome::Failed(message) => (JobState::Failed, Some(message.as_str())),
        };
        let changed = self.connection.execute(
            "UPDATE jobs SET state = ?1, finished_at = ?2, detail = ?3
             WHERE kind = ?4 AND key = ?5 AND owner = ?6 AND state = ?7",
            params![
                state.code(),
                now,
                detail,
                kind,
                key,
                owner,
                JobState::Claimed.code()
            ],
        )?;
        if changed == 0 {
            return Err(Error::JobNotOwned {
                kind: kind.to_string(),
                key: key.to_string(),
            });
        }
        Ok(())
    }
}

impl Reader {
    /// The current row for `(kind, key)`, for callers waiting on another process's job.
    pub fn job(&self, kind: &str, key: &str) -> Result<Option<Job>> {
        let row = self
            .connection
            .query_row(
                "SELECT state, owner, claimed_at, finished_at, detail FROM jobs
                 WHERE kind = ?1 AND key = ?2",
                [kind, key],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(state, owner, claimed_at, finished_at, detail)| {
            Ok(Job {
                kind: kind.to_string(),
                key: key.to_string(),
                state: decode_state(state)?,
                owner,
                claimed_at,
                finished_at,
                detail,
            })
        })
        .transpose()
    }
}

fn decode_state(code: i64) -> Result<JobState> {
    JobState::from_code(code).ok_or_else(|| Error::CorruptValue(format!("job state {code}")))
}
