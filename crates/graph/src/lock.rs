//! The indexer lock: one process at a time may bulk-load the graph.

use std::fs::{File, OpenOptions, TryLockError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::layout::GraphDir;

/// How long [`IndexerLock::acquire`] keeps trying before it reports the lock as held. Generous
/// against a descheduled [`IndexerLock::is_held`] probe on a busy machine, yet short enough that
/// a process which loses to a real indexer gives up at once.
const ACQUIRE_DEADLINE: Duration = Duration::from_millis(500);
const ACQUIRE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Proof that this process is the indexer; the lock is released on drop or process exit,
/// including a crash, so a dead indexer never blocks the next one.
#[derive(Debug)]
pub struct IndexerLock {
    _file: File,
}

impl IndexerLock {
    /// Takes the lock or fails with [`Error::IndexerLockHeld`] once the deadline passes.
    /// The deadline outlasts a [`IndexerLock::is_held`] probe, which holds the lock only
    /// for an instant, so probing readers never make an idle graph look busy.
    pub fn acquire(dir: &GraphDir) -> Result<Self> {
        dir.create()?;
        let file = open_lock_file(dir)?;
        let deadline = Instant::now() + ACQUIRE_DEADLINE;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(IndexerLock { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    sleep(ACQUIRE_POLL_INTERVAL);
                }
                Err(TryLockError::WouldBlock) => return Err(Error::IndexerLockHeld),
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }

    /// Whether some process is indexing now. The probe takes a shared lock for an instant;
    /// [`IndexerLock::acquire`] retries past it.
    pub fn is_held(dir: &GraphDir) -> Result<bool> {
        if !dir.lock_path().exists() {
            return Ok(false);
        }
        match File::open(dir.lock_path())?.try_lock_shared() {
            Ok(()) => Ok(false),
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

fn open_lock_file(dir: &GraphDir) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.lock_path())
}
