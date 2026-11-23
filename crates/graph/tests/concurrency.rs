mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use common::*;
use graph::{Claim, Error, IndexerLock, JobOutcome, Query, Reader, Writer};

const NOW: i64 = 1_700_000_000;
const LEASE: i64 = 60;
/// Probes come from query responses, so they are frequent but never a busy loop; a busy loop
/// would keep one probe preempted while it holds the shared lock for longer than the
/// acquire grace period.
const PROBE_INTERVAL: Duration = Duration::from_micros(100);

#[test]
fn readers_keep_reading_while_the_writer_loads() {
    let scratch = TestDir::new("readers-during-write");
    let dir = scratch.graph_dir();
    let lock = IndexerLock::acquire(&dir).unwrap();
    let mut writer = Writer::open(&dir).unwrap();
    writer
        .load(&lock, vec![chain_file("seed.rs", "seed", 3)], 1, NOW)
        .unwrap();

    const FILES: usize = 300;
    const CHAIN: u32 = 20;
    let writing = AtomicBool::new(true);
    let reads_during_write = thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let reader = Reader::open(&dir).unwrap();
                    let (mut seen_during_write, mut last_files) = (0, 0);
                    while writing.load(Ordering::Acquire) {
                        let files = reader.files().unwrap().len();
                        assert!(files >= last_files, "a reader saw the graph shrink");
                        last_files = files;
                        reader.coverage(5).unwrap();
                        reader.find("seed", 3).unwrap();
                        seen_during_write += 1;
                    }
                    seen_during_write
                })
            })
            .collect();
        let batches = (0..FILES)
            .map(|index| chain_file(&format!("f{index}.rs"), &format!("fn{index}_"), CHAIN));
        writer.load(&lock, batches, 10, NOW).unwrap();
        writing.store(false, Ordering::Release);
        readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .sum::<usize>()
    });

    assert!(
        reads_during_write > 0,
        "readers never ran while the writer was loading"
    );
    let reader = Reader::open(&dir).unwrap();
    assert_eq!(reader.files().unwrap().len(), FILES + 1);
    let edge_total: u64 = reader
        .coverage(0)
        .unwrap()
        .edges
        .iter()
        .map(|count| count.count)
        .sum();
    assert_eq!(edge_total, FILES as u64 * (CHAIN as u64 - 1) + 2);
}

#[test]
fn only_one_process_can_be_the_indexer() {
    let scratch = TestDir::new("lock");
    let dir = scratch.graph_dir();
    assert!(!IndexerLock::is_held(&dir).unwrap());

    let first = IndexerLock::acquire(&dir).unwrap();
    assert!(IndexerLock::is_held(&dir).unwrap());
    assert!(matches!(
        IndexerLock::acquire(&dir),
        Err(Error::IndexerLockHeld)
    ));
    thread::scope(|scope| {
        let from_another_thread = scope.spawn(|| IndexerLock::acquire(&dir)).join().unwrap();
        assert!(matches!(from_another_thread, Err(Error::IndexerLockHeld)));
    });

    Writer::open(&dir).unwrap();
    let reader = Reader::open(&dir).unwrap();
    assert!(reader.freshness().unwrap().indexing_in_progress);

    drop(first);
    assert!(!reader.freshness().unwrap().indexing_in_progress);
    IndexerLock::acquire(&dir).unwrap();
}

#[test]
fn freshness_probes_do_not_make_an_idle_graph_look_busy() {
    let scratch = TestDir::new("probe-race");
    let dir = scratch.graph_dir();
    Writer::open(&dir).unwrap();
    let probing = AtomicBool::new(true);
    thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while probing.load(Ordering::Relaxed) {
                    IndexerLock::is_held(&dir).unwrap();
                    thread::sleep(PROBE_INTERVAL);
                }
            });
        }
        // Stops the probes even when an acquire panics: the scope joins them before it
        // lets the panic out, so a probe that never stops would hang the test binary.
        let _stop_probes = StopOnDrop(&probing);
        for _ in 0..50 {
            drop(IndexerLock::acquire(&dir).unwrap());
        }
    });
}

struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

#[test]
fn opening_an_existing_graph_does_not_wait_for_the_indexer_write_lock() {
    let scratch = TestDir::new("open-during-write");
    let dir = scratch.graph_dir();
    Writer::open(&dir).unwrap();
    let indexer = rusqlite::Connection::open(dir.database_path()).unwrap();
    indexer.execute_batch("BEGIN IMMEDIATE").unwrap();
    Writer::open(&dir).unwrap();
}

#[test]
fn simultaneous_claims_give_the_job_to_exactly_one_owner() {
    let scratch = TestDir::new("claims");
    let dir = scratch.graph_dir();
    Writer::open(&dir).unwrap();

    let claims: Vec<Claim> = thread::scope(|scope| {
        let handles: Vec<_> = (0..30)
            .map(|agent| {
                let dir = dir.clone();
                scope.spawn(move || {
                    let mut writer = Writer::open(&dir).unwrap();
                    writer
                        .claim_job(
                            "index",
                            "stale-set-1",
                            &format!("agent-{agent}"),
                            LEASE,
                            NOW,
                        )
                        .unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });

    let winners = claims
        .iter()
        .filter(|claim| **claim == Claim::Acquired)
        .count();
    assert_eq!(winners, 1);
    assert!(
        claims
            .iter()
            .all(|claim| matches!(claim, Claim::Acquired | Claim::HeldBy { .. }))
    );
}

#[test]
fn claims_expire_finish_and_can_be_retried() {
    let scratch = TestDir::new("job-lifecycle");
    let dir = scratch.graph_dir();
    let mut writer = Writer::open(&dir).unwrap();
    let reader = Reader::open(&dir).unwrap();

    assert_eq!(
        writer
            .claim_job("describe", "hash1", "a", LEASE, NOW)
            .unwrap(),
        Claim::Acquired
    );
    assert_eq!(
        writer
            .claim_job("describe", "hash1", "b", LEASE, NOW + 1)
            .unwrap(),
        Claim::HeldBy {
            owner: "a".to_string()
        }
    );
    assert!(matches!(
        writer.finish_job("describe", "hash1", "b", &JobOutcome::Done, NOW + 2),
        Err(Error::JobNotOwned { .. })
    ));

    assert_eq!(
        writer
            .claim_job("describe", "hash1", "b", LEASE, NOW + LEASE)
            .unwrap(),
        Claim::Acquired
    );
    writer
        .finish_job(
            "describe",
            "hash1",
            "b",
            &JobOutcome::Failed("backend down".into()),
            NOW + 70,
        )
        .unwrap();
    let job = reader.job("describe", "hash1").unwrap().unwrap();
    assert_eq!(
        (job.state, job.detail.as_deref()),
        (graph::JobState::Failed, Some("backend down"))
    );

    assert_eq!(
        writer
            .claim_job("describe", "hash1", "c", LEASE, NOW + 71)
            .unwrap(),
        Claim::Acquired
    );
    writer
        .finish_job("describe", "hash1", "c", &JobOutcome::Done, NOW + 72)
        .unwrap();
    assert_eq!(
        writer
            .claim_job("describe", "hash1", "d", LEASE, NOW + 73)
            .unwrap(),
        Claim::AlreadyDone
    );
    assert_eq!(
        writer
            .claim_job("describe", "hash2", "d", LEASE, NOW + 73)
            .unwrap(),
        Claim::Acquired
    );
    assert!(reader.job("describe", "missing").unwrap().is_none());
}
