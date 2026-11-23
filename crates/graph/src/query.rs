//! The read seam. The query crate builds budgets, ranking and text output on this trait, so a
//! shared server can replace the file-backed [`crate::Reader`] later without touching callers.

use serde::Serialize;

use crate::components::{Component, ComponentId};
use crate::csr::Direction;
use crate::error::Result;
use crate::facts::FileTags;
use crate::id::SymbolId;
use crate::kinds::{EdgeKind, FileStatus, NodeKind, Provenance};
use crate::store::UnixSeconds;

/// Edge kinds that mean "depends on", the default for impact analysis.
pub const DEPENDENCY_KINDS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Inherits,
    EdgeKind::Implements,
    EdgeKind::Reads,
    EdgeKind::Writes,
    EdgeKind::HandlesRoute,
    EdgeKind::RegistersHook,
    EdgeKind::FiresHook,
    EdgeKind::Declared,
];

/// A node with the context rankers and printers need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeInfo {
    pub id: SymbolId,
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    /// Path of the defining file; `None` for global nodes such as unresolved calls.
    pub file: Option<String>,
    pub line: Option<u32>,
    pub component: Option<ComponentId>,
    /// Number of edges touching the node, in either direction.
    pub degree: u32,
    pub tags: FileTags,
}

/// Where an edge was seen and how sure the indexer is of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdgeInfo {
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub file: String,
    pub line: Option<u32>,
    pub reason: Option<String>,
}

/// A node one edge away from the node asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Neighbour {
    pub node: NodeInfo,
    /// `Out` when the asked-about node is the edge's source.
    pub direction: Direction,
    pub edge: EdgeInfo,
}

/// Neighbours ordered most-connected first, cut at the caller's limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Neighbours {
    pub items: Vec<Neighbour>,
    /// How many exist before the cut, for the footer.
    pub total: usize,
}

/// One node on a path and the edge that led to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hop {
    pub node: NodeInfo,
    /// `None` for the first node.
    pub via: Option<PathStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PathStep {
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub line: Option<u32>,
    /// False when the edge was followed against its direction.
    pub forward: bool,
}

/// A node reached by an impact search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reached {
    pub node: NodeInfo,
    pub depth: u32,
}

/// Everything that depends, directly or not, on a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Impact {
    /// Nearest first, cut at the caller's limit.
    pub reached: Vec<Reached>,
    /// How many were reached before the cut.
    pub total: usize,
    /// True when the slice hit its node cap, so more may exist.
    pub truncated: bool,
}

/// Files of one language in one state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileCount {
    pub language: Option<String>,
    pub status: FileStatus,
    pub count: u64,
}

/// Edges of one kind and provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdgeCount {
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub count: u64,
}

/// A file that was not parsed cleanly, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProblemFile {
    pub path: String,
    pub status: FileStatus,
    pub detail: Option<String>,
}

/// The raw numbers behind the `coverage` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoverageInputs {
    pub files: Vec<FileCount>,
    pub edges: Vec<EdgeCount>,
    /// Calls that resolved to no definition.
    pub unresolved_edges: u64,
    /// Up to the requested number of files that are not `parsed`, in path order.
    pub problem_files: Vec<ProblemFile>,
    /// How many files are not `parsed` in total.
    pub problem_file_total: u64,
}

/// How current the graph is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Freshness {
    /// When the last load committed; `None` if nothing was ever loaded.
    pub indexed_at: Option<UnixSeconds>,
    /// Another process holds the indexer lock; the data served is the last committed snapshot.
    pub indexing_in_progress: bool,
}

/// What the query layer needs from a graph. Every method reads one consistent snapshot.
/// An empty `kinds` slice means every edge kind.
pub trait Query {
    /// Resolves text to nodes, best first. Accepts `@<hex prefix of an id>`, `path:line`, an exact
    /// qualified name, or fuzzy words; the first form that matches decides. Unresolved nodes are
    /// never returned.
    fn find(&self, text: &str, limit: usize) -> Result<Vec<NodeInfo>>;

    fn node(&self, id: SymbolId) -> Result<Option<NodeInfo>>;

    /// Direct neighbours, most connected first.
    fn neighbours(
        &self,
        id: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        limit: usize,
    ) -> Result<Neighbours>;

    /// The fewest-hop path, `from` first, or `None` when none exists within `max_depth`.
    /// Paths with `Both` can pass through an unresolved node and join unrelated callers.
    fn path(
        &self,
        from: SymbolId,
        to: SymbolId,
        direction: Direction,
        kinds: &[EdgeKind],
        max_depth: u32,
    ) -> Result<Option<Vec<Hop>>>;

    /// Everything that reaches `id` over `kinds` (use [`DEPENDENCY_KINDS`] for "what breaks").
    fn impact(
        &self,
        id: SymbolId,
        kinds: &[EdgeKind],
        max_depth: u32,
        limit: usize,
    ) -> Result<Impact>;

    fn coverage(&self, problem_file_limit: usize) -> Result<CoverageInputs>;

    fn freshness(&self) -> Result<Freshness>;

    /// Children of `parent`, or the top level for `None`.
    fn components(&self, parent: Option<ComponentId>) -> Result<Vec<Component>>;

    /// The most connected symbols under a component and all its descendants.
    fn top_symbols(&self, component: ComponentId, limit: usize) -> Result<Vec<NodeInfo>>;
}
