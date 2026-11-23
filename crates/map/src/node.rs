//! The side panel's view of one node: description, key symbols and the matching command.

use describe::Overrides;
use graph::{ComponentId, Freshness, NodeInfo, Query, Reader, UnixSeconds};
use query::{Find, Overview, Page};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::labels::labels;
use crate::node_id::NodeId;

const KEY_SYMBOL_COUNT: usize = 10;

#[derive(Debug, Serialize)]
pub struct SymbolView {
    pub kind: String,
    pub qualified_name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct NodeView {
    pub id: NodeId,
    pub kind: &'static str,
    pub name: String,
    pub level: Option<u32>,
    /// `None` until a description exists; the map then offers to write one.
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub description_stale: bool,
    pub stale: bool,
    pub written_at: Option<UnixSeconds>,
    pub key_symbols: Vec<SymbolView>,
    /// What an agent would type to get the same information.
    pub command: String,
    pub freshness: Freshness,
}

pub fn node(reader: &Reader, overrides: &Overrides, id: NodeId) -> Result<NodeView> {
    match id {
        NodeId::Component(component) => component_node(reader, overrides, component),
        NodeId::File(file) => file_node(reader, file),
    }
}

fn component_node(reader: &Reader, overrides: &Overrides, id: ComponentId) -> Result<NodeView> {
    let component = reader
        .component(id)?
        .ok_or_else(|| Error::NotFound(format!("component {}", id.0)))?;
    let shown = labels(reader, overrides, &component)?;
    let command = Overview {
        component: Some(id),
        page: Page::default(),
    }
    .command();
    Ok(NodeView {
        id: NodeId::Component(id),
        kind: "component",
        level: Some(component.level),
        stale: shown.is_stale(&component),
        description_stale: shown.description_stale,
        written_at: shown.written_at,
        tags: shown.tags,
        summary: shown.summary,
        name: shown.name,
        key_symbols: reader
            .top_symbols(id, KEY_SYMBOL_COUNT)?
            .iter()
            .map(symbol_view)
            .collect(),
        command,
        freshness: reader.freshness()?,
    })
}

fn file_node(reader: &Reader, id: i64) -> Result<NodeView> {
    let file = reader
        .files()?
        .into_iter()
        .find(|file| file.id == id)
        .ok_or_else(|| Error::NotFound(format!("file {id}")))?;
    let command = Find {
        text: file.path.clone(),
        page: Page::default(),
    }
    .command();
    Ok(NodeView {
        id: NodeId::File(id),
        kind: "file",
        name: file.path,
        level: None,
        summary: None,
        tags: file.language.into_iter().collect(),
        description_stale: false,
        stale: false,
        written_at: None,
        key_symbols: Vec::new(),
        command,
        freshness: reader.freshness()?,
    })
}

fn symbol_view(symbol: &NodeInfo) -> SymbolView {
    SymbolView {
        kind: symbol.kind.to_string(),
        qualified_name: symbol.qualified_name.clone(),
        file: symbol.file.clone(),
        line: symbol.line,
    }
}
