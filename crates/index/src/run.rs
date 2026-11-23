//! One indexing run, start to finish. I/O happens here and in the walker and the per-file stage;
//! everything between is pure.
//!
//! Memory stays bounded by never holding what a run does not need: the per-file stage keeps only
//! the small part of each extraction other files can see (the full extractions go to the
//! extraction cache on disk), and facts are built one file at a time from the cache as the
//! loader consumes them.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use graph::{
    CachedExtraction, DEFAULT_BATCH_FILES, ExtractionCache, FileFacts, FileRow, FileStamp,
    GraphDir, IndexerLock, LoadStats, Reader, UnixSeconds, Writer,
};
use languages::{Extraction, LanguageViews, ProjectContext, ProjectView, ResolvedEdge};
use rayon::prelude::*;

use crate::cache::{CacheReader, CachedHashes};
use crate::config::Config;
use crate::error::Result;
use crate::facts::{facts_with_declared_edges, facts_with_extraction, facts_without_extraction};
use crate::links;
use crate::plan::{Change, changed_keys, follow_includes, mentions_any};
use crate::process::{Extracted, Inputs, Inspected, Worker, inspect, parsed_record};
use crate::walk::{ScannedFile, scan};

/// How long a file must have been left alone before its modification time is believed. Longer
/// than the coarsest timestamp granularity a filesystem is likely to have.
const MODIFICATION_TIME_TRUST_DELAY_NS: i64 = 2_000_000_000;
/// Extractions waiting for the cache writer; bounds the memory they hold.
const CACHE_QUEUE_LENGTH: usize = 256;

pub struct IndexOptions {
    pub batch_files: usize,
    /// Stamped on the graph as its age; a parameter so time stays at the edge.
    pub now: UnixSeconds,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            batch_files: DEFAULT_BATCH_FILES,
            now: graph::now(),
        }
    }
}

/// What a run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexReport {
    /// Files the graph now describes.
    pub files: usize,
    /// Files read and parsed again because their content or settings changed.
    pub parsed: usize,
    /// Unchanged files resolved again because a name or file they refer to changed.
    pub re_resolved: usize,
    pub unchanged: usize,
    pub deleted: usize,
    /// `.cartograph/` directories below the root, skipped.
    pub nested_graphs: Vec<String>,
    pub warnings: Vec<String>,
    pub loaded: LoadStats,
}

/// Indexes the tree at `root` into its `.cartograph/` graph. Only files whose content (or
/// surroundings) changed since the last run are parsed; the resulting graph equals what a clean
/// run would produce. Fails at once with the graph's lock error when another process is indexing.
pub fn index_directory(root: &Path, options: &IndexOptions) -> Result<IndexReport> {
    let dir = GraphDir::of_root(root);
    let config = Config::load(&dir)?;
    let lock = IndexerLock::acquire(&dir)?;
    let mut writer = Writer::open(&dir)?;
    let previous: HashMap<String, FileRow> = Reader::open(&dir)?
        .files()?
        .into_iter()
        .map(|row| (row.path.clone(), row))
        .collect();

    let mut scan = scan(root, &config)?;
    let (context, context_warnings) = crate::context::read_context(root, &scan);
    let mut cache = ExtractionCache::open(&dir, &fingerprint(&context, &config))?;
    let cached = cache.hashes()?;
    cache.mark_run_started()?;

    let inputs = Inputs {
        config: &config,
        context: &context,
        previous: &previous,
        cached: &cached,
        trusted_before_ns: trusted_before_ns(),
    };
    let scanned_files: Vec<&ScannedFile> = scan.files.iter().chain(&scan.nested_graphs).collect();
    let mut inspected = inspect_all(&scanned_files, &inputs, &mut cache, &dir)?;
    inspected.sort_by(|left, right| left.record.path.cmp(&right.record.path));

    let deleted: Vec<String> = {
        let current = paths_of(&inspected);
        previous
            .keys()
            .filter(|path| !current.contains(path.as_str()))
            .cloned()
            .collect()
    };

    let reader = CacheReader::open(&dir)?;
    let anything_changed = !deleted.is_empty() || inspected.iter().any(|entry| !entry.unchanged);
    if anything_changed {
        load_shared_extractions(&mut inspected, &dir)?;
    }
    let current = paths_of(&inspected);
    let project_files = shared_files(&inspected);
    let language_views = LanguageViews::new(&context, project_files.iter().copied());

    let changed: Vec<&Inspected> = inspected
        .iter()
        .filter(|entry| !entry.is_unchanged())
        .collect();
    let rewritten = changed.iter().map(|entry| Change {
        path: entry.record.path.as_str(),
        old: entry.previous_shared.as_ref(),
        new: entry.shared.as_ref(),
    });
    let removed_old: Vec<Option<Extraction>> = deleted
        .iter()
        .map(|path| reader.shared(path))
        .collect::<Result<_>>()?;
    let removed = deleted.iter().zip(&removed_old).map(|(path, old)| Change {
        path: path.as_str(),
        old: old.as_ref(),
        new: None,
    });
    let keys = changed_keys(&rewritten.chain(removed).collect::<Vec<_>>());
    let unchanged_files: Vec<(&str, &Extraction)> = inspected
        .iter()
        .filter(|entry| entry.is_unchanged())
        .filter_map(|entry| Some((entry.record.path.as_str(), entry.shared.as_ref()?)))
        .collect();
    let affected = affected_files(keys, &unchanged_files, &dir)?;

    let to_write: Vec<&Inspected> = inspected
        .iter()
        .filter(|entry| !entry.is_unchanged() || affected.contains(entry.record.path.as_str()))
        .collect();

    let declared_links: Vec<(&str, &[links::Link])> = to_write
        .iter()
        .filter_map(|entry| match &entry.extracted {
            Extracted::Declared(declared) => {
                Some((entry.record.path.as_str(), declared.as_slice()))
            }
            _ => None,
        })
        .collect();
    let (declared_edges, link_problems) =
        resolve_declared_links(&declared_links, &current, &context, &project_files);

    let removed_files = writer.remove_files(&lock, &deleted, options.now)?;
    let loaded = load_streaming(
        &mut writer,
        &lock,
        &to_write,
        Loading {
            views: &language_views,
            declared_edges: &declared_edges,
            dir: &dir,
        },
        options,
    )?;

    let report = IndexReport {
        files: inspected.len(),
        parsed: changed.iter().filter(|entry| entry.is_fresh()).count(),
        re_resolved: to_write.len() - changed.len(),
        unchanged: inspected.len() - to_write.len(),
        deleted: removed_files,
        nested_graphs: scan
            .nested_graphs
            .iter()
            .map(|file| file.path.clone())
            .collect(),
        warnings: std::mem::take(&mut scan.warnings)
            .into_iter()
            .chain(context_warnings)
            .chain(link_problems)
            .collect(),
        loaded,
    };
    refresh_stamps(&mut writer, &lock, &inspected, &previous)?;
    drop(language_views);
    forget_uncached(&mut cache, &cached, &inspected)?;
    cache.mark_run_finished()?;
    Ok(report)
}

/// The instant before which a file's modification time proves it was left alone.
fn trusted_before_ns() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_nanos()).ok())
        .unwrap_or(0);
    now - MODIFICATION_TIME_TRUST_DELAY_NS
}

/// Looks at every file on the rayon pool while one thread stores the new extractions in the cache.
fn inspect_all(
    files: &[&ScannedFile],
    inputs: &Inputs<'_>,
    cache: &mut ExtractionCache,
    dir: &GraphDir,
) -> Result<Vec<Inspected>> {
    let (sender, receiver) = mpsc::sync_channel::<CachedExtraction>(CACHE_QUEUE_LENGTH);
    let (inspected, stored) = std::thread::scope(|scope| {
        let storing = scope.spawn(move || cache.store(receiver));
        let inspected: Result<Vec<Inspected>> = files
            .par_iter()
            .map_init(
                || Worker {
                    // The cache file was created by this run a moment ago.
                    reader: CacheReader::open(dir).expect("the extraction cache opens"),
                    extractions: sender.clone(),
                },
                |worker, file| inspect(file, inputs, worker),
            )
            .collect();
        drop(sender);
        (inspected, storing.join())
    });
    match stored {
        Ok(stored) => stored?,
        Err(panic) => std::panic::resume_unwind(panic),
    }
    inspected
}

/// Fills in what other files see of each reused file, from the cache.
fn load_shared_extractions(inspected: &mut [Inspected], dir: &GraphDir) -> Result<()> {
    inspected
        .par_iter_mut()
        .filter(|entry| matches!(entry.extracted, Extracted::Reused))
        .try_for_each_init(
            || CacheReader::open(dir).expect("the extraction cache opens"),
            |reader, entry| {
                let shared = reader
                    .shared(&entry.record.path)?
                    .ok_or_else(|| crate::Error::CacheEntryMissing(entry.record.path.clone()))?;
                entry.record = parsed_record(entry.record.clone(), shared.syntax_errors);
                entry.shared = Some(shared);
                Ok(())
            },
        )
}

fn paths_of(inspected: &[Inspected]) -> HashSet<&str> {
    inspected
        .iter()
        .map(|entry| entry.record.path.as_str())
        .collect()
}

/// Every file with an extraction, in path order.
fn shared_files(inspected: &[Inspected]) -> Vec<(&str, &Extraction)> {
    inspected
        .iter()
        .filter_map(|entry| Some((entry.record.path.as_str(), entry.shared.as_ref()?)))
        .collect()
}

/// Unchanged files that must be resolved again because something they refer to moved: those that
/// reach a changed key through include chains, and those that mention a key by name anywhere.
///
/// ponytail: the by-name check reads every unchanged file's cached extraction whenever a
/// definition changed; keep a per-file name index in the cache if that shows up in a benchmark.
fn affected_files<'a>(
    keys: HashSet<String>,
    unchanged: &[(&'a str, &Extraction)],
    dir: &GraphDir,
) -> Result<HashSet<&'a str>> {
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let (keys, mut affected) = follow_includes(keys, unchanged);
    let by_name: Vec<&'a str> = unchanged
        .par_iter()
        .map_init(
            || CacheReader::open(dir).expect("the extraction cache opens"),
            |reader, (path, _)| {
                let extraction = reader.full(path)?;
                Ok(mentions_any(&extraction, &keys).then_some(*path))
            },
        )
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    affected.extend(by_name);
    Ok(affected)
}

/// Declared edges of the links files being written. They resolve names against the whole
/// project, a view only they need, so it is built only when there is a links file.
fn resolve_declared_links(
    declared_links: &[(&str, &[links::Link])],
    current: &HashSet<&str>,
    context: &ProjectContext,
    project_files: &[(&str, &Extraction)],
) -> (HashMap<String, Vec<ResolvedEdge>>, Vec<String>) {
    if declared_links.is_empty() {
        return Default::default();
    }
    let project = ProjectView::new(context, project_files.iter().copied());
    links::declared_edges(declared_links.iter().copied(), current, &project)
}

/// What fact building reads besides the file itself.
struct Loading<'a> {
    views: &'a LanguageViews<'a>,
    /// Edges of each links file, by its path.
    declared_edges: &'a HashMap<String, Vec<ResolvedEdge>>,
    dir: &'a GraphDir,
}

/// Builds facts on the rayon pool and loads them on this thread as they arrive, so memory holds
/// one batch of facts rather than the whole graph.
fn load_streaming(
    writer: &mut Writer,
    lock: &IndexerLock,
    entries: &[&Inspected],
    loading: Loading<'_>,
    options: &IndexOptions,
) -> Result<LoadStats> {
    let (sender, receiver) = mpsc::sync_channel::<FileFacts>(options.batch_files.max(1));
    let (loaded, produced) = std::thread::scope(|scope| {
        let producer = scope.spawn(move || {
            entries.par_iter().try_for_each_init(
                || CacheReader::open(loading.dir).expect("the extraction cache opens"),
                |reader, entry| {
                    let facts = facts_of(entry, reader, &loading)?;
                    // A closed channel means the loader already failed; its error is reported.
                    let _ = sender.send(facts);
                    Ok::<(), crate::Error>(())
                },
            )
        });
        let loaded = writer.load(lock, receiver, options.batch_files, options.now);
        (loaded, producer.join())
    });
    match produced {
        Ok(produced) => produced?,
        Err(panic) => std::panic::resume_unwind(panic),
    }
    Ok(loaded?)
}

fn facts_of(entry: &Inspected, reader: &CacheReader, loading: &Loading<'_>) -> Result<FileFacts> {
    let record = entry.record.clone();
    let Some(module) = entry.module.filter(|_| entry.is_cacheable()) else {
        return Ok(match loading.declared_edges.get(&record.path) {
            Some(edges) => facts_with_declared_edges(record, edges.clone()),
            None => facts_without_extraction(record),
        });
    };
    let extraction = reader.full(&record.path)?;
    let project = loading
        .views
        .for_module(module)
        .expect("a file with an extraction belongs to a family with a view");
    Ok(facts_with_extraction(record, &extraction, module, project))
}

/// Records the new size and modification time of files found unchanged by content, so the next
/// run can skip reading them.
fn refresh_stamps(
    writer: &mut Writer,
    lock: &IndexerLock,
    inspected: &[Inspected],
    previous: &HashMap<String, FileRow>,
) -> Result<()> {
    let stamps: Vec<FileStamp> = inspected
        .iter()
        .filter(|entry| entry.is_unchanged())
        .filter(|entry| {
            previous.get(&entry.record.path).is_some_and(|row| {
                row.size_bytes != entry.record.size_bytes || row.mtime_ns != entry.record.mtime_ns
            })
        })
        .map(|entry| FileStamp {
            path: &entry.record.path,
            size_bytes: entry.record.size_bytes,
            mtime_ns: entry.record.mtime_ns,
        })
        .collect();
    Ok(writer.refresh_stamps(lock, &stamps)?)
}

/// Drops cache entries for files that are gone or were not extracted this run, so a stale
/// extraction is never taken for what the graph holds.
fn forget_uncached(
    cache: &mut ExtractionCache,
    cached: &CachedHashes,
    inspected: &[Inspected],
) -> Result<()> {
    let keep: HashSet<&str> = inspected
        .iter()
        .filter(|entry| entry.is_cacheable())
        .map(|entry| entry.record.path.as_str())
        .collect();
    let forgotten: Vec<&str> = cached
        .keys()
        .map(String::as_str)
        .filter(|path| !keep.contains(path))
        .collect();
    Ok(cache.remove(&forgotten)?)
}

/// What the cached extractions depend on besides file content: the project context and the
/// settings that tag files.
fn fingerprint(context: &ProjectContext, config: &Config) -> String {
    let json = serde_json::to_vec(&(context, config)).expect("settings serialise to JSON");
    graph::ContentHash::of_bytes(&json).to_string()
}
