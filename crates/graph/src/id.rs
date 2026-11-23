//! Stable identifiers: symbol ids survive re-indexing because they are hashes, not row numbers.

use std::fmt;

use serde::{Serialize, Serializer};

use crate::kinds::NodeKind;

const ID_BITS: u32 = 63;
const HEX_DIGITS: usize = 16;

/// Identity of a node: a 63-bit hash of kind, qualified name and file path, so it fits a
/// positive SQLite integer and doubles as the node's primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(u64);

impl SymbolId {
    /// Derives the id. The file path is ignored for global kinds (see [`NodeKind::is_global`]).
    /// Overloads that share a qualified name must be told apart by the caller inside the name.
    pub fn derive(kind: NodeKind, qualified_name: &str, file_path: &str) -> Self {
        let file_path = if kind.is_global() { "" } else { file_path };
        let mut hasher = blake3::Hasher::new();
        hasher.update(&[kind as u8]);
        hasher.update(qualified_name.as_bytes());
        hasher.update(&[0]);
        hasher.update(file_path.as_bytes());
        let digest = hasher.finalize();
        let eight_bytes: [u8; 8] = digest.as_bytes()[..8]
            .try_into()
            .expect("digest has 32 bytes");
        SymbolId(u64::from_le_bytes(eight_bytes) >> (64 - ID_BITS))
    }

    /// The database key. Always non-negative.
    pub fn as_i64(self) -> i64 {
        self.0 as i64
    }

    /// Inverse of [`Self::as_i64`]; `None` for a negative value no id can have.
    pub fn from_i64(value: i64) -> Option<Self> {
        u64::try_from(value).ok().map(SymbolId)
    }

    /// The first `digits` hex digits, the short form agents pass back as `@a3f9`.
    pub fn short(self, digits: usize) -> String {
        self.to_string()[..digits.min(HEX_DIGITS)].to_string()
    }

    /// The inclusive id range whose hex form starts with `prefix`; `None` if `prefix` is not hex,
    /// is empty or is longer than an id.
    pub fn range_for_prefix(prefix: &str) -> Option<(i64, i64)> {
        if prefix.is_empty() || prefix.len() > HEX_DIGITS {
            return None;
        }
        let value = u64::from_str_radix(prefix, 16).ok()?;
        let free_bits = 4 * (HEX_DIGITS - prefix.len()) as u32;
        let low = value << free_bits;
        let high = low | ((1u64 << free_bits) - 1);
        (high >> ID_BITS == 0).then_some((low as i64, high as i64))
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016x}", self.0)
    }
}

impl Serialize for SymbolId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// BLAKE3 hash of a file's bytes or of a component's members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of_bytes(bytes: &[u8]) -> Self {
        ContentHash(*blake3::hash(bytes).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(ContentHash)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
