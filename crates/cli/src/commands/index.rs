use std::path::Path;

use graph::{GraphDir, Writer};
use index::{IndexOptions, IndexReport, index_directory};
use serde_json::json;

use crate::Result;

pub fn run(root: &Path, as_json: bool) -> Result<String> {
    let report = index_directory(root, &IndexOptions::default())?;
    let dir = GraphDir::of_root(root);
    crate::stale_components::mark(&dir, &mut Writer::open(&dir)?)?;
    Ok(if as_json {
        serde_json::to_string(&to_json(&report))?
    } else {
        text(&report)
    })
}

fn text(report: &IndexReport) -> String {
    let mut lines = vec![format!(
        "indexed {} files: {} parsed, {} re-resolved, {} unchanged, {} deleted; wrote {} nodes and {} edges",
        report.files,
        report.parsed,
        report.re_resolved,
        report.unchanged,
        report.deleted,
        report.loaded.nodes,
        report.loaded.edges,
    )];
    lines.extend(
        report
            .nested_graphs
            .iter()
            .map(|path| format!("skipped nested graph: {path}")),
    );
    lines.extend(
        report
            .warnings
            .iter()
            .map(|warning| format!("warning: {warning}")),
    );
    lines.join("\n")
}

fn to_json(report: &IndexReport) -> serde_json::Value {
    json!({
        "files": report.files,
        "parsed": report.parsed,
        "re_resolved": report.re_resolved,
        "unchanged": report.unchanged,
        "deleted": report.deleted,
        "nested_graphs": report.nested_graphs,
        "warnings": report.warnings,
        "loaded": {
            "files": report.loaded.files,
            "nodes": report.loaded.nodes,
            "edges": report.loaded.edges,
        },
    })
}
