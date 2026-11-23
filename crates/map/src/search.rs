//! Search box results: components by name, then symbols by the graph's fuzzy lookup.

use graph::{Query, Reader};
use serde::Serialize;

use crate::error::Result;
use crate::node_id::NodeId;

const COMPONENT_HITS: usize = 10;
const SYMBOL_HITS: usize = 15;

#[derive(Debug, Serialize)]
pub struct Hit {
    pub kind: &'static str,
    /// The component to open: the component itself, or the one holding the symbol.
    pub target: NodeId,
    pub label: String,
    pub detail: String,
}

pub fn search(reader: &Reader, text: &str) -> Result<Vec<Hit>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = component_hits(reader, text)?;
    for symbol in reader.find(text, SYMBOL_HITS)? {
        let Some(component) = symbol.component else {
            continue;
        };
        hits.push(Hit {
            kind: "symbol",
            target: NodeId::Component(component),
            label: symbol.qualified_name,
            detail: match (symbol.file, symbol.line) {
                (Some(file), Some(line)) => format!("{file}:{line}"),
                (Some(file), None) => file,
                (None, _) => symbol.kind.to_string(),
            },
        });
    }
    Ok(hits)
}

// ponytail: walks the whole component tree per keystroke; add a name index to the graph if a
// tree of tens of thousands of components makes search slow.
fn component_hits(reader: &Reader, text: &str) -> Result<Vec<Hit>> {
    let needle = text.to_lowercase();
    let mut hits = Vec::new();
    let mut pending = reader.components(None)?;
    while let Some(component) = pending.pop() {
        pending.extend(reader.components(Some(component.id))?);
        if component.name.to_lowercase().contains(&needle) {
            hits.push(Hit {
                kind: "component",
                target: NodeId::Component(component.id),
                label: component.name,
                detail: format!("level {}", component.level),
            });
        }
    }
    hits.sort_by(|a, b| a.label.cmp(&b.label));
    hits.truncate(COMPONENT_HITS);
    Ok(hits)
}
