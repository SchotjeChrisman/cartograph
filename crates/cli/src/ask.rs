//! Running one query against the graph at a root: the path shared by the CLI and the MCP tools.

use std::path::Path;

use graph::{GraphDir, Reader};
use query::{Engine, Output, Request};

use crate::{Result, auto_index, orientation};

/// Opens the graph, runs the staleness check, answers `request`, and records that the caller is
/// now oriented.
pub fn answer(root: &Path, request: &Request) -> Result<Output> {
    let dir = GraphDir::of_root(root);
    let reader = Reader::open(&dir)?;
    let output = Engine::new(&reader, graph::now())
        .with_staleness_check(|| auto_index::check(root))
        .run(request)?;
    if let Err(error) = orientation::mark(&dir) {
        eprintln!("warning: could not record the query for the strict hook: {error}");
    }
    Ok(output)
}

/// The dense text, or the full structure as one line of JSON.
pub fn render(output: &Output, as_json: bool) -> Result<String> {
    if as_json {
        Ok(serde_json::to_string(output)?)
    } else {
        Ok(query::render::text(output))
    }
}
