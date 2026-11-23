//! The structured answer behind both the JSON output and the text renderer.

use graph::{
    Component, CoverageInputs, Description, Direction, EdgeKind, NodeInfo, PathStep, Provenance,
};
use serde::Serialize;

use crate::request::Relation;

/// A node plus the two names agents act on: the short id to pass back, and its component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeRef {
    /// Shortest hex prefix of the id that no other node shares, usable as `@<short_id>`.
    pub short_id: String,
    #[serde(flatten)]
    pub node: NodeInfo,
    pub component_name: Option<String>,
}

/// What follows the cut: how many results were left out and the command that shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct More {
    pub remaining: usize,
    /// False when the search itself was capped, so `remaining` is a lower bound.
    pub exact: bool,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdgeSummary {
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub file: String,
    pub line: Option<u32>,
    pub reason: Option<String>,
}

/// Ranked candidates for a name, from `find` or from an ambiguous target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidates {
    pub query: String,
    pub candidates: Vec<NodeRef>,
    pub more: Option<More>,
    /// Set when another tool asked for one node and the name matched several.
    pub ambiguous_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Related {
    pub node: NodeRef,
    /// Hops from the focus.
    pub depth: u32,
    /// `Out` when the node on the near side is the edge's source.
    pub direction: Direction,
    pub edge: EdgeSummary,
    /// The nearer node this one was reached from; `None` at depth 1.
    pub via: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExploreReport {
    pub focus: NodeRef,
    pub relation: Relation,
    pub depth: u32,
    /// Everything found, not only what fits the budget; a lower bound when `more.exact` is false.
    pub total: usize,
    pub related: Vec<Related>,
    pub more: Option<More>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HopRef {
    pub node: NodeRef,
    pub via: Option<PathStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathReport {
    pub from: NodeRef,
    pub to: NodeRef,
    pub max_depth: u32,
    /// Edges on the path; `None` when none exists within `max_depth`.
    pub hop_count: Option<usize>,
    /// The nodes shown, with the middle cut when the budget is small.
    pub hops: Option<Vec<HopRef>>,
    /// Hops cut from the middle to fit the budget, shown as a gap after this index.
    pub omitted: Option<Omitted>,
    pub more: Option<More>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Omitted {
    pub after_index: usize,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reachable {
    pub node: NodeRef,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImpactReport {
    pub target: NodeRef,
    pub max_depth: u32,
    /// Everything reached, not only what fits the budget.
    pub total: usize,
    /// How many were reached at each depth, nearest first.
    pub by_depth: Vec<(u32, usize)>,
    /// The search hit its size cap, so more may exist.
    pub truncated: bool,
    pub reached: Vec<Reachable>,
    pub more: Option<More>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentView {
    pub component: Component,
    /// `None` until `describe` has run for it.
    pub description: Option<Description>,
    /// The description was written for an older version of the component.
    pub description_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverviewReport {
    /// The component whose children are listed; `None` at the top level.
    pub parent: Option<ComponentView>,
    pub components: Vec<ComponentView>,
    /// The most connected symbols under `parent`.
    pub key_symbols: Vec<NodeRef>,
    pub more: Option<More>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoverageReport {
    /// `problem_files` holds only the slice that fits the budget.
    pub inputs: CoverageInputs,
    /// Calls that resolved to a definition over all calls, 0 to 1; `None` without calls.
    pub call_resolution: Option<f64>,
    pub more: Option<More>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Report {
    Overview(OverviewReport),
    Find(Candidates),
    Explore(ExploreReport),
    Path(PathReport),
    Impact(ImpactReport),
    Coverage(CoverageReport),
    /// An ambiguous target, answered with the candidates to choose from.
    Ambiguous(Candidates),
    /// A target or component that matched nothing.
    NotFound {
        query: String,
    },
}

/// One answer: notes about the graph's freshness, then the report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Output {
    pub notes: Vec<String>,
    pub report: Report,
}
