//! Walker and parallel parsing: turns a directory tree into graph facts and loads them.
//!
//! The run is: walk (gitignore-aware, secrets and nested graphs set aside) -> per file, in
//! parallel, read + hash + extract through a [`languages::LanguageModule`] -> resolve references
//! across files -> stream the facts into the graph store. Unchanged files, by content hash, are
//! neither re-parsed nor rewritten unless something they refer to changed, and the result always
//! equals a clean full index.

mod cache;
mod classify;
mod config;
mod context;
mod error;
mod facts;
mod links;
mod plan;
mod process;
mod run;
mod walk;

pub use config::Config;
pub use error::{Error, Result};
pub use run::{IndexOptions, IndexReport, index_directory};
