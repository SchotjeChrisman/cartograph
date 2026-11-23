mod common;

use common::TestDir;
use graph::{CachedExtraction, ContentHash, ExtractionCache};

fn entry(path: &str) -> CachedExtraction {
    CachedExtraction {
        path: path.to_string(),
        hash: ContentHash::of_bytes(path.as_bytes()),
        full: format!("full of {path}").into_bytes(),
        shared: format!("shared of {path}").into_bytes(),
    }
}

#[test]
fn entries_come_back_and_reflect_the_latest_store() {
    let dir = TestDir::new("cache-roundtrip");
    let mut cache = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    cache.store([entry("a.c"), entry("b.c")]).unwrap();
    cache
        .store([CachedExtraction {
            full: b"newer".to_vec(),
            ..entry("a.c")
        }])
        .unwrap();

    let reader = ExtractionCache::open_reader(&dir.graph_dir()).unwrap();
    assert_eq!(reader.full("a.c").unwrap().unwrap(), b"newer");
    assert_eq!(reader.shared("b.c").unwrap().unwrap(), b"shared of b.c");
    assert_eq!(reader.full("missing.c").unwrap(), None);
    assert_eq!(cache.hashes().unwrap().len(), 2);

    cache.remove(&["a.c"]).unwrap();
    assert_eq!(cache.hashes().unwrap().len(), 1);
}

#[test]
fn a_different_fingerprint_empties_the_cache() {
    let dir = TestDir::new("cache-fingerprint");
    let mut cache = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    cache.store([entry("a.c")]).unwrap();
    drop(cache);

    let same = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    assert_eq!(same.hashes().unwrap().len(), 1);
    drop(same);
    let changed = ExtractionCache::open(&dir.graph_dir(), "settings-b").unwrap();
    assert!(changed.hashes().unwrap().is_empty());
}

#[test]
fn a_run_that_never_finished_empties_the_cache() {
    let dir = TestDir::new("cache-unfinished");
    let mut cache = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    cache.mark_run_started().unwrap();
    cache.store([entry("a.c")]).unwrap();
    drop(cache);

    let after_crash = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    assert!(after_crash.hashes().unwrap().is_empty());

    after_crash.mark_run_started().unwrap();
    after_crash.mark_run_finished().unwrap();
    drop(after_crash);
    let mut after_success = ExtractionCache::open(&dir.graph_dir(), "settings-a").unwrap();
    after_success.store([entry("b.c")]).unwrap();
    assert_eq!(after_success.hashes().unwrap().len(), 1);
}
