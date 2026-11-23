//! Per-file work, run in parallel: look at the file, decide whether it changed, extract.

use std::collections::HashMap;
use std::fs::Metadata;
use std::sync::mpsc::SyncSender;
use std::time::UNIX_EPOCH;

use graph::{CachedExtraction, ContentHash, FileRecord, FileRow, FileStatus};
use languages::{Extraction, LanguageModule, ProjectContext, SourceFile};

use crate::cache::{CacheReader, CachedHashes, encode};
use crate::classify::{Classification, tags_for};
use crate::config::Config;
use crate::error::Result;
use crate::links::{self, Link};
use crate::walk::ScannedFile;

/// The language name recorded for a links file.
const LINKS_LANGUAGE: &str = "links";

/// Everything the per-file stage reads besides the file itself.
pub struct Inputs<'a> {
    pub config: &'a Config,
    pub context: &'a ProjectContext,
    pub previous: &'a HashMap<String, FileRow>,
    pub cached: &'a CachedHashes,
    /// A file modified at or after this instant (ns since the epoch) could be edited again
    /// without its modification time moving, so its time is never trusted to prove it unchanged.
    pub trusted_before_ns: i64,
}

/// What one worker thread owns: its own read connection to the cache and the way to add to it.
pub struct Worker {
    pub reader: CacheReader,
    pub extractions: SyncSender<CachedExtraction>,
}

pub enum Extracted {
    /// Parsed in this run.
    Fresh,
    /// Hash and settings match the last run; the cached extraction still stands.
    Reused,
    /// A `links.yaml`: its declared links, resolved later against the whole project.
    Declared(Vec<Link>),
    /// Nothing was parsed: secret, unsupported language, nested graph or read failure.
    None,
}

pub struct Inspected {
    pub record: FileRecord,
    pub extracted: Extracted,
    pub module: Option<&'static dyn LanguageModule>,
    /// The graph already holds exactly this file record and nothing was extracted that could differ.
    pub unchanged: bool,
    /// What other files see of this file's extraction: set when it was parsed in this run, and
    /// loaded from the cache for a reused file when the run needs it.
    pub shared: Option<Extraction>,
    /// What the cache held for a changed file before this run replaced it.
    pub previous_shared: Option<Extraction>,
}

impl Inspected {
    fn new(
        record: FileRecord,
        extracted: Extracted,
        module: Option<&'static dyn LanguageModule>,
        unchanged: bool,
    ) -> Self {
        Inspected {
            record,
            extracted,
            module,
            unchanged,
            shared: None,
            previous_shared: None,
        }
    }

    pub fn is_unchanged(&self) -> bool {
        self.unchanged
    }

    pub fn is_fresh(&self) -> bool {
        matches!(self.extracted, Extracted::Fresh)
    }

    /// Whether the cache should keep an entry for this file.
    pub fn is_cacheable(&self) -> bool {
        matches!(self.extracted, Extracted::Fresh | Extracted::Reused)
    }
}

/// Looks at one file; a file parsed now has its extraction cached before this returns.
pub fn inspect(file: &ScannedFile, inputs: &Inputs<'_>, worker: &Worker) -> Result<Inspected> {
    let (mut inspected, fresh) = look(file, inputs);
    if !inspected.unchanged && inputs.cached.contains_key(&file.path) {
        // Read before the replacement is stored: the old shape is what other files were resolved against.
        inspected.previous_shared = worker.reader.shared(&file.path)?;
    }
    if let (Some(extraction), Some(hash)) = (fresh, inspected.record.content_hash) {
        let entry = encode(&file.path, hash, &extraction);
        inspected.shared = Some(extraction.visible_to_other_files());
        // A closed channel means the cache writer already failed; its error is reported.
        let _ = worker.extractions.send(entry);
    }
    Ok(inspected)
}

/// The file's verdict, plus the extraction when one was made now.
fn look(file: &ScannedFile, inputs: &Inputs<'_>) -> (Inspected, Option<Extraction>) {
    let base = FileRecord {
        path: file.path.clone(),
        repo: file.repo.clone(),
        language: None,
        content_hash: None,
        size_bytes: 0,
        mtime_ns: None,
        status: FileStatus::Unparsed,
        status_detail: None,
        tags: tags_for(&file.path, None, inputs.config),
    };
    let unread = |status, language: Option<&str>, detail: String| {
        let record = FileRecord {
            language: language.map(str::to_string),
            status,
            status_detail: Some(detail),
            ..base.clone()
        };
        Inspected::new(
            record.clone(),
            Extracted::None,
            None,
            matches_previous(&record, inputs),
        )
    };
    match &file.classification {
        Classification::Secret { pattern } => (
            unread(
                FileStatus::SkippedSecret,
                None,
                format!("matches secret pattern `{pattern}`"),
            ),
            None,
        ),
        Classification::NestedGraph => (
            unread(
                FileStatus::SkippedNestedGraph,
                None,
                "directory holds a graph of its own".to_string(),
            ),
            None,
        ),
        Classification::Unsupported(language) => (
            unread(
                FileStatus::Unparsed,
                Some(language),
                format!("no {language} language module yet"),
            ),
            None,
        ),
        Classification::Parseable(module) => look_at_source(file, *module, base, inputs),
        Classification::Declarations => (inspect_declarations(file, base, inputs), None),
    }
}

/// Reads a links file. It is cheap to read and its edges depend on the rest of the project, so
/// it is never reported as unchanged.
fn inspect_declarations(file: &ScannedFile, base: FileRecord, inputs: &Inputs<'_>) -> Inspected {
    let language = Some(LINKS_LANGUAGE.to_string());
    let failed = |detail: String, record: FileRecord| {
        Inspected::new(
            FileRecord {
                status: FileStatus::ParseFailed,
                status_detail: Some(detail),
                ..record
            },
            Extracted::None,
            None,
            false,
        )
    };
    let bytes = match std::fs::read(&file.absolute) {
        Ok(bytes) => bytes,
        Err(error) => {
            return failed(
                format!("could not read file: {error}"),
                FileRecord { language, ..base },
            );
        }
    };
    let record = FileRecord {
        language,
        content_hash: Some(ContentHash::of_bytes(&bytes)),
        size_bytes: bytes.len() as u64,
        tags: tags_for(&file.path, Some(&bytes), inputs.config),
        ..base
    };
    let parsed = std::str::from_utf8(&bytes)
        .map_err(|error| error.to_string())
        .and_then(links::parse);
    match parsed {
        Ok(declared) => Inspected::new(
            FileRecord {
                status: FileStatus::Parsed,
                ..record
            },
            Extracted::Declared(declared),
            None,
            false,
        ),
        Err(message) => failed(message, record),
    }
}

fn look_at_source(
    file: &ScannedFile,
    module: &'static dyn LanguageModule,
    base: FileRecord,
    inputs: &Inputs<'_>,
) -> (Inspected, Option<Extraction>) {
    let language = Some(module.name().to_string());
    // Stat before reading: an edit in between leaves an older time with newer content, which the
    // next run notices as a changed time, never the reverse.
    let stamp = std::fs::metadata(&file.absolute).map(|metadata| Stamp::of(&metadata, inputs));
    if let Some(record) = stamp
        .as_ref()
        .ok()
        .and_then(|stamp| unchanged_by_stamp(&base, module, stamp, inputs))
    {
        return (
            Inspected::new(record, Extracted::Reused, Some(module), true),
            None,
        );
    }
    let read = stamp.and_then(|stamp| Ok((std::fs::read(&file.absolute)?, stamp)));
    let (bytes, stamp) = match read {
        Ok(read) => read,
        Err(error) => {
            let record = FileRecord {
                language,
                status: FileStatus::ParseFailed,
                status_detail: Some(format!("could not read file: {error}")),
                ..base
            };
            return (
                Inspected::new(record, Extracted::None, Some(module), false),
                None,
            );
        }
    };
    let hash = ContentHash::of_bytes(&bytes);
    let read_record = FileRecord {
        language,
        content_hash: Some(hash),
        size_bytes: bytes.len() as u64,
        mtime_ns: stamp.mtime_ns,
        tags: tags_for(&file.path, Some(&bytes), inputs.config),
        ..base
    };
    if still_valid(&read_record, inputs) {
        return (
            Inspected::new(
                parsed_record(read_record, 0),
                Extracted::Reused,
                Some(module),
                true,
            ),
            None,
        );
    }
    let source = SourceFile {
        path: &file.path,
        bytes: &bytes,
        context: inputs.context,
    };
    match module.extract(&source) {
        Ok(extraction) => (
            Inspected::new(
                parsed_record(read_record, extraction.syntax_errors),
                Extracted::Fresh,
                Some(module),
                false,
            ),
            Some(extraction),
        ),
        Err(error) => {
            let record = FileRecord {
                status: FileStatus::ParseFailed,
                status_detail: Some(error.to_string()),
                ..read_record
            };
            (
                Inspected::new(record, Extracted::None, Some(module), false),
                None,
            )
        }
    }
}

/// What a file's metadata says about it.
struct Stamp {
    size_bytes: u64,
    /// `None` when the file is too recent for its time to prove anything.
    mtime_ns: Option<i64>,
}

impl Stamp {
    fn of(metadata: &Metadata, inputs: &Inputs<'_>) -> Self {
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .and_then(|elapsed| i64::try_from(elapsed.as_nanos()).ok())
            .filter(|mtime_ns| *mtime_ns < inputs.trusted_before_ns);
        Stamp {
            size_bytes: metadata.len(),
            mtime_ns,
        }
    }
}

/// The record of a file whose size and modification time are those the graph recorded when it
/// hashed it, and whose extraction is cached: it needs no reading at all.
fn unchanged_by_stamp(
    base: &FileRecord,
    module: &dyn LanguageModule,
    stamp: &Stamp,
    inputs: &Inputs<'_>,
) -> Option<FileRecord> {
    let previous = inputs.previous.get(&base.path)?;
    let hash = previous.content_hash?;
    let same_stamp = stamp.mtime_ns.is_some()
        && previous.mtime_ns == stamp.mtime_ns
        && previous.size_bytes == stamp.size_bytes;
    let record = parsed_record(
        FileRecord {
            language: Some(module.name().to_string()),
            content_hash: Some(hash),
            size_bytes: stamp.size_bytes,
            mtime_ns: stamp.mtime_ns,
            tags: previous.tags,
            ..base.clone()
        },
        0,
    );
    let extraction_cached = inputs.cached.get(&base.path) == Some(&hash);
    (same_stamp && extraction_cached && matches_previous(&record, inputs)).then_some(record)
}

/// `syntax_errors` fills the detail text; a reused file gets its real count when its cached
/// extraction is loaded.
pub fn parsed_record(record: FileRecord, syntax_errors: u32) -> FileRecord {
    FileRecord {
        status: FileStatus::Parsed,
        status_detail: (syntax_errors > 0)
            .then(|| format!("{syntax_errors} syntax error nodes; kept what parsed")),
        ..record
    }
}

/// Whether the graph and the cache already describe this exact file.
fn still_valid(record: &FileRecord, inputs: &Inputs<'_>) -> bool {
    let hash = record.content_hash;
    hash.is_some()
        && inputs.cached.get(&record.path) == hash.as_ref()
        && matches_previous(&parsed_record(record.clone(), 0), inputs)
}

/// Whether the graph's row for this path agrees with `record` on everything the graph can be
/// compared by without reading its detail text.
fn matches_previous(record: &FileRecord, inputs: &Inputs<'_>) -> bool {
    inputs.previous.get(&record.path).is_some_and(|previous| {
        previous.status == record.status
            && previous.content_hash == record.content_hash
            && previous.repo == record.repo
            && previous.language == record.language
            && previous.tags == record.tags
    })
}
