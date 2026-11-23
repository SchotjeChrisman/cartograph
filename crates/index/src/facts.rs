//! Turns one file's record, extraction and resolved edges into the facts the graph loads.

use std::collections::HashSet;

use graph::{EdgeFact, FileFacts, FileRecord, FileStatus, NodeFact, NodeKind};
use languages::{
    Endpoint, Extraction, LanguageModule, ProjectView, ResolvedEdge, containment_edges, file_name,
};

/// Facts for a file with no extraction: only the record and, unless the file must stay unread,
/// its file node.
pub fn facts_without_extraction(record: FileRecord) -> FileFacts {
    let nodes = if matches!(
        record.status,
        FileStatus::SkippedSecret | FileStatus::SkippedNestedGraph
    ) {
        Vec::new()
    } else {
        vec![file_node(&record.path)]
    };
    FileFacts {
        file: record,
        nodes,
        edges: Vec::new(),
    }
}

pub fn facts_with_extraction(
    record: FileRecord,
    extraction: &Extraction,
    module: &dyn LanguageModule,
    project: &ProjectView<'_>,
) -> FileFacts {
    let path = record.path.clone();
    let mut resolved = containment_edges(&path, extraction);
    resolved.extend(module.resolve(&path, extraction, project));

    let mut nodes = vec![file_node(&path)];
    nodes.extend(extraction.definitions.iter().map(|definition| NodeFact {
        kind: definition.kind,
        name: definition.name.clone(),
        qualified_name: definition.qualified_name.clone(),
        line: Some(definition.line),
        end_line: Some(definition.end_line),
        signature: definition.signature.clone(),
        doc: definition.doc.clone(),
    }));
    nodes.extend(global_nodes(&resolved));
    let edges = distinct(resolved.into_iter().map(edge_fact));
    FileFacts {
        file: record,
        nodes,
        edges,
    }
}

/// Facts for a file that holds nothing but edges it declares, such as `links.yaml`.
pub fn facts_with_declared_edges(record: FileRecord, edges: Vec<ResolvedEdge>) -> FileFacts {
    let mut nodes = vec![file_node(&record.path)];
    nodes.extend(global_nodes(&edges));
    FileFacts {
        file: record,
        nodes,
        edges: distinct(edges.into_iter().map(edge_fact)),
    }
}

fn file_node(path: &str) -> NodeFact {
    NodeFact {
        kind: NodeKind::File,
        name: file_name(path).to_string(),
        qualified_name: path.to_string(),
        line: None,
        end_line: None,
        signature: None,
        doc: None,
    }
}

/// One node per distinct unresolved name, table, config key or hook an edge ends at, so the
/// edges have something to point at. Global nodes belong to no file, so their location is left
/// out: the same node arrives from every file that mentions it and must look the same each time.
fn global_nodes(edges: &[ResolvedEdge]) -> Vec<NodeFact> {
    let mut seen = HashSet::new();
    edges
        .iter()
        .flat_map(|edge| [&edge.src, &edge.dst])
        .filter_map(|end| match end {
            Endpoint::Unresolved(name) => Some((NodeKind::Unresolved, name.as_str())),
            Endpoint::Definition {
                kind,
                qualified_name,
                ..
            } if kind.is_global() => Some((*kind, qualified_name.as_str())),
            _ => None,
        })
        .filter(|(kind, name)| seen.insert((*kind, *name)))
        .map(|(kind, qualified_name)| NodeFact {
            kind,
            name: global_node_name(kind, qualified_name),
            qualified_name: qualified_name.to_string(),
            line: None,
            end_line: None,
            signature: None,
            doc: None,
        })
        .collect()
}

/// The short name shown for a global node: the last segment of a qualified name.
fn global_node_name(kind: NodeKind, qualified_name: &str) -> String {
    let separator = match kind {
        NodeKind::Unresolved => "::",
        NodeKind::Hook => "\\",
        _ => return qualified_name.to_string(),
    };
    qualified_name
        .rsplit(separator)
        .next()
        .unwrap_or(qualified_name)
        .to_string()
}

/// Drops edges that repeat another in everything the graph stores, such as two names imported
/// by one `use` line from the same file.
fn distinct(edges: impl Iterator<Item = EdgeFact>) -> Vec<EdgeFact> {
    let mut seen = HashSet::new();
    edges
        .filter(|edge| seen.insert((edge.src, edge.dst, edge.kind, edge.line)))
        .collect()
}

fn edge_fact(edge: ResolvedEdge) -> EdgeFact {
    EdgeFact {
        src: edge.src.symbol_id(),
        dst: edge.dst.symbol_id(),
        kind: edge.kind,
        provenance: edge.provenance,
        line: Some(edge.line),
        reason: edge.reason,
    }
}
