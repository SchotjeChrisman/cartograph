//! Plain-English descriptions of components, written by a pluggable model backend.
//!
//! Lazy and top-down: [`Describer::describe_down`] fills the levels asked for and reuses any
//! description whose component hash still matches. Index and map work without any of this.

mod backend;
mod error;
mod fake;
mod output;
mod overrides;
mod prompt;
mod service;
mod source;

pub use backend::{Backend, BackendError, ProcessBackend};
pub use error::{Error, Result};
pub use fake::FakeBackend;
pub use output::{Generated, OutputError, parse_reply};
pub use overrides::{Override, Overrides, OverridesError};
pub use prompt::{
    ChildSummary, ComponentLink, ComponentSlice, DEFAULT_TOKEN_CAP, SymbolLine, build_prompt,
};
pub use service::{CLAIM_LEASE_SECONDS, Describer, JOB_KIND, Outcome};
pub use source::{GraphSliceSource, SliceSource};
