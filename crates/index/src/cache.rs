//! Encoding extractions into, and decoding them out of, the blobs of the graph crate's
//! extraction cache. A cached extraction lets an incremental run resolve a file whose bytes did
//! not change without parsing it again.

use std::collections::HashMap;

use graph::{CachedExtraction, ContentHash, ExtractionCache, GraphDir};
use languages::Extraction;

use crate::error::{Error, Result};

pub fn encode(path: &str, hash: ContentHash, extraction: &Extraction) -> CachedExtraction {
    CachedExtraction {
        path: path.to_string(),
        hash,
        full: to_json(extraction),
        shared: to_json(&extraction.visible_to_other_files()),
    }
}

fn to_json(extraction: &Extraction) -> Vec<u8> {
    serde_json::to_vec(extraction).expect("an extraction serialises to JSON")
}

fn decode(path: &str, bytes: &[u8]) -> Result<Extraction> {
    serde_json::from_slice(bytes).map_err(|source| Error::CacheEntry {
        path: path.to_string(),
        source,
    })
}

/// One thread's read connection to the cache.
pub struct CacheReader(ExtractionCache);

impl CacheReader {
    pub fn open(dir: &GraphDir) -> Result<Self> {
        Ok(CacheReader(ExtractionCache::open_reader(dir)?))
    }

    /// What other files can see of `path`, if the cache holds it.
    pub fn shared(&self, path: &str) -> Result<Option<Extraction>> {
        self.0
            .shared(path)?
            .map(|bytes| decode(path, &bytes))
            .transpose()
    }

    /// Everything extracted from `path`; the cache must hold it.
    pub fn full(&self, path: &str) -> Result<Extraction> {
        match self.0.full(path)? {
            Some(bytes) => decode(path, &bytes),
            None => Err(Error::CacheEntryMissing(path.to_string())),
        }
    }
}

/// The hash each cached file was extracted from.
pub type CachedHashes = HashMap<String, ContentHash>;
