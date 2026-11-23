//! Typed graph model and SQLite store.
//!
//! One writer loads facts in batches while any number of readers query the last committed
//! snapshot. [`Query`] is the read seam the query crate builds on; [`Reader`] implements it over
//! the graph file.

mod components;
mod coverage;
mod csr;
mod error;
mod extraction_cache;
mod facts;
mod files;
mod id;
mod jobs;
mod kinds;
mod layout;
mod loader;
mod lock;
mod names;
mod query;
mod read;
mod rows;
mod schema;
mod slice;
mod store;

pub use components::{Component, ComponentId, Description};
pub use csr::{Csr, Direction, Link, SliceEdge, Visit};
pub use error::{Error, Result};
pub use extraction_cache::{CachedExtraction, ExtractionCache};
pub use facts::{EdgeFact, FileFacts, FileRecord, FileTags, NodeFact};
pub use files::{FileLink, FileRow};
pub use id::{ContentHash, SymbolId};
pub use jobs::{Claim, Job, JobOutcome};
pub use kinds::{EdgeKind, FileStatus, JobState, NodeKind, Provenance};
pub use layout::GraphDir;
pub use loader::{DEFAULT_BATCH_FILES, FileStamp, LoadStats};
pub use lock::IndexerLock;
pub use names::split_words;
pub use query::{
    CoverageInputs, DEPENDENCY_KINDS, EdgeCount, EdgeInfo, FileCount, Freshness, Hop, Impact,
    Neighbour, Neighbours, NodeInfo, PathStep, ProblemFile, Query, Reached,
};
pub use slice::SLICE_NODE_CAP;
pub use store::{Reader, UnixSeconds, Writer, now};
