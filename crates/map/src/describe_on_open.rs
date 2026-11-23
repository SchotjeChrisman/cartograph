//! Writing a component's description when a person opens it in the map.

use std::process;

use describe::{Backend, DEFAULT_TOKEN_CAP, Describer, GraphSliceSource, Outcome, Overrides};
use graph::{GraphDir, Reader, Writer, now};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::node::{NodeView, node};
use crate::node_id::NodeId;

#[derive(Debug, Serialize)]
pub struct Described {
    /// `cached`, `generated`, `overridden` or `in_progress` (another process is writing it).
    pub outcome: &'static str,
    pub node: NodeView,
}

/// Describes the component unless a current description exists; the model backend runs inside
/// this call, so it can take minutes.
pub fn describe_component(
    dir: &GraphDir,
    backend: &dyn Backend,
    overrides: &Overrides,
    id: NodeId,
) -> Result<Described> {
    let NodeId::Component(component_id) = id else {
        return Err(Error::BadRequest(
            "only components are described, not files".to_string(),
        ));
    };
    let reader = Reader::open(dir)?;
    let component = reader
        .component(component_id)?
        .ok_or_else(|| Error::NotFound(format!("component {}", component_id.0)))?;
    let source = GraphSliceSource::load(&reader)?;
    let describer = Describer {
        backend,
        source: &source,
        overrides,
        owner: format!("map-{}", process::id()),
        token_cap: DEFAULT_TOKEN_CAP,
    };
    let outcome = describer.describe(&reader, &mut Writer::open(dir)?, &component, now())?;
    Ok(Described {
        outcome: match outcome {
            Outcome::Cached => "cached",
            Outcome::Generated => "generated",
            Outcome::Overridden => "overridden",
            Outcome::InProgress { .. } => "in_progress",
        },
        // A new reader: the one above predates the description just written.
        node: node(&Reader::open(dir)?, overrides, id)?,
    })
}
