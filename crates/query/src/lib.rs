//! Budgeted, ranked agent queries over the graph.
//!
//! [`Engine`] answers the six tools (`overview`, `find`, `explore`, `path`, `impact`, `coverage`)
//! from any graph that implements [`graph::Query`] and [`ComponentSource`]. An answer is an
//! [`Output`]: structured data for `--json`, and [`render::text`] for the dense agent-facing text.
//! Both are cut at the request's token budget and carry the exact follow-up command.

mod budget;
mod coverage;
mod engine;
mod error;
mod explore;
mod find;
mod fresh;
mod impact;
mod overview;
mod path;
pub mod render;
mod report;
mod request;
mod resolve;
mod source;

pub use budget::estimate_tokens;
pub use engine::Engine;
pub use error::{Error, Result};
pub use fresh::{Staleness, StalenessCheck};
pub use report::{
    Candidates, ComponentView, CoverageReport, EdgeSummary, ExploreReport, HopRef, ImpactReport,
    More, NodeRef, Omitted, Output, OverviewReport, PathReport, Reachable, Related, Report,
};
pub use request::{
    Coverage, DEFAULT_BUDGET, DEFAULT_DEPTH, DEFAULT_IMPACT_DEPTH, DEFAULT_PATH_DEPTH, Explore,
    Find, Impact, Overview, Page, PathRequest, Relation, Request,
};
pub use source::ComponentSource;
