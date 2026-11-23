//! The dense plain-text form of an answer: one line per node or edge, `file:line` on every
//! node, a footer with the follow-up command. Pure functions of the report, so goldens are stable.

use std::fmt::Write as _;

use graph::{CoverageInputs, Direction, EdgeKind, Provenance};

use crate::report::{
    Candidates, ComponentView, CoverageReport, ExploreReport, HopRef, ImpactReport, More, NodeRef,
    Output, OverviewReport, PathReport, Reachable, Related, Report,
};
use crate::request::Relation;

const SUMMARY_SENTENCE_END: &str = ". ";

pub fn text(output: &Output) -> String {
    let mut text = String::new();
    for note in &output.notes {
        let _ = writeln!(text, "{note}");
    }
    match &output.report {
        Report::Overview(report) => overview(&mut text, report),
        Report::Find(report) | Report::Ambiguous(report) => candidates(&mut text, report),
        Report::Explore(report) => explore(&mut text, report),
        Report::Path(report) => path(&mut text, report),
        Report::Impact(report) => impact(&mut text, report),
        Report::Coverage(report) => coverage(&mut text, report),
        Report::NotFound { query } => {
            let _ = writeln!(text, "nothing found for `{query}`");
        }
    }
    text
}

fn overview(text: &mut String, report: &OverviewReport) {
    let _ = writeln!(
        text,
        "{}",
        overview_header(report.parent.as_ref(), report.components.len())
    );
    for component in &report.components {
        let _ = writeln!(text, "{}", component_line(component));
    }
    if !report.key_symbols.is_empty() {
        let _ = writeln!(text, "key symbols:");
    }
    for symbol in &report.key_symbols {
        let _ = writeln!(text, "{}", node_line(symbol, true));
    }
    footer(text, report.more.as_ref());
}

fn candidates(text: &mut String, report: &Candidates) {
    let _ = writeln!(
        text,
        "{}",
        candidates_header(&report.query, report.ambiguous_for.as_deref())
    );
    if report.candidates.is_empty() {
        let _ = writeln!(text, "(no matches; try fewer or different words)");
    }
    for candidate in &report.candidates {
        let _ = writeln!(text, "{}", node_line(candidate, true));
    }
    footer(text, report.more.as_ref());
}

fn explore(text: &mut String, report: &ExploreReport) {
    let header = explore_header(&report.focus, report.relation, report.depth, report.total);
    let _ = writeln!(text, "{header}");
    if report.related.is_empty() {
        let _ = writeln!(text, "(none)");
    }
    for related in &report.related {
        let _ = writeln!(text, "{}", related_line(related));
    }
    footer(text, report.more.as_ref());
}

fn path(text: &mut String, report: &PathReport) {
    let header = path_header(&report.from, &report.to, report.max_depth, report.hop_count);
    let _ = writeln!(text, "{header}");
    let leading = report
        .omitted
        .map_or(usize::MAX, |omitted| omitted.after_index);
    for (index, hop) in report.hops.iter().flatten().enumerate() {
        if index == leading {
            let count = report.omitted.map_or(0, |omitted| omitted.count);
            let _ = writeln!(text, "   ... {count} nodes omitted ...");
        }
        let _ = writeln!(text, "{}", hop_line(hop));
    }
    footer(text, report.more.as_ref());
}

fn impact(text: &mut String, report: &ImpactReport) {
    let header = impact_header(
        &report.target,
        report.total,
        &report.by_depth,
        report.truncated,
    );
    let _ = writeln!(text, "{header}");
    for reached in &report.reached {
        let _ = writeln!(text, "{}", reachable_line(reached));
    }
    footer(text, report.more.as_ref());
}

fn coverage(text: &mut String, report: &CoverageReport) {
    let _ = writeln!(
        text,
        "{}",
        coverage_summary(&report.inputs, report.call_resolution)
    );
    for file in &report.inputs.problem_files {
        let _ = writeln!(text, "{}", problem_file_line(file));
    }
    footer(text, report.more.as_ref());
}

fn footer(text: &mut String, more: Option<&More>) {
    if let Some(more) = more {
        let bound = if more.exact { "" } else { "at least " };
        let _ = writeln!(
            text,
            "+{bound}{} more. Next: {}",
            more.remaining, more.command
        );
    }
}

pub fn candidates_header(query: &str, ambiguous_for: Option<&str>) -> String {
    match ambiguous_for {
        Some(tool) => format!("{tool} `{query}`: ambiguous, repeat with one of these ids"),
        None => format!("find `{query}`:"),
    }
}

/// `@id kind name file:line [component]`, with the edge count when `with_edges` is set.
pub fn node_line(node: &NodeRef, with_edges: bool) -> String {
    let info = &node.node;
    let mut line = format!("@{} {} {}", node.short_id, info.kind, info.qualified_name);
    match (&info.file, info.line) {
        (Some(file), Some(number)) => {
            let _ = write!(line, "  {file}:{number}");
        }
        (Some(file), None) => {
            let _ = write!(line, "  {file}");
        }
        (None, _) => {}
    }
    if let Some(component) = &node.component_name {
        let _ = write!(line, "  [{component}]");
    }
    if info.tags.vendored {
        line.push_str("  vendored");
    }
    if info.tags.generated {
        line.push_str("  generated");
    }
    if with_edges {
        let _ = write!(line, "  edges={}", info.degree);
    }
    line
}

pub fn explore_header(focus: &NodeRef, relation: Relation, depth: u32, total: usize) -> String {
    let what = match relation {
        Relation::Callers => "callers of",
        Relation::Callees => "callees of",
        Relation::Contains => "contents of",
        Relation::All => "neighbours of",
    };
    format!(
        "{what} {} (depth {depth}, {total} found)",
        node_line(focus, false)
    )
}

pub fn related_line(related: &Related) -> String {
    let arrow = match related.direction {
        Direction::In => "<-",
        _ => "->",
    };
    let indent = "  ".repeat(related.depth.saturating_sub(1) as usize);
    let mut line = format!("{indent}{arrow} {}", node_line(&related.node, false));
    let edge = &related.edge;
    let _ = write!(line, "  {}", edge.kind);
    if let Some(number) = edge.line {
        let _ = write!(line, "@{}:{number}", edge.file);
    }
    line.push_str(&provenance_suffix(edge.provenance, edge.reason.as_deref()));
    if let Some(via) = &related.via {
        let _ = write!(line, "  via @{via}");
    }
    line
}

fn provenance_suffix(provenance: Provenance, reason: Option<&str>) -> String {
    match (provenance, reason) {
        (Provenance::Extracted, _) => String::new(),
        (other, Some(reason)) => format!("  {other}({reason})"),
        (other, None) => format!("  {other}"),
    }
}

pub fn impact_header(
    target: &NodeRef,
    total: usize,
    by_depth: &[(u32, usize)],
    truncated: bool,
) -> String {
    let depths: Vec<String> = by_depth
        .iter()
        .map(|(depth, count)| format!("d{depth}={count}"))
        .collect();
    let cap = if truncated { ", search capped" } else { "" };
    format!(
        "impact of {}: {total} affected ({}{cap})",
        node_line(target, false),
        depths.join(" ")
    )
}

pub fn reachable_line(reached: &Reachable) -> String {
    format!("d{} {}", reached.depth, node_line(&reached.node, false))
}

pub fn path_header(
    from: &NodeRef,
    to: &NodeRef,
    max_depth: u32,
    hop_count: Option<usize>,
) -> String {
    match hop_count {
        Some(count) => format!(
            "path @{} -> @{}: {} hops",
            from.short_id, to.short_id, count
        ),
        None => format!(
            "no path from @{} {} to @{} {} within {max_depth} hops; try --depth",
            from.short_id, from.node.qualified_name, to.short_id, to.node.qualified_name
        ),
    }
}

pub fn hop_line(hop: &HopRef) -> String {
    let node = node_line(&hop.node, false);
    let Some(step) = hop.via else {
        return format!("   {node}");
    };
    let mark = match provenance_suffix(step.provenance, None).trim() {
        "" => String::new(),
        text => format!(" {text}"),
    };
    let edge = match step.forward {
        true => format!("-{}{mark}->", step.kind),
        false => format!("<-{}{mark}-", step.kind),
    };
    format!("{edge} {node}")
}

pub fn overview_header(parent: Option<&ComponentView>, child_count: usize) -> String {
    match parent {
        Some(parent) => format!("{}: children {child_count}", component_line(parent)),
        None if child_count == 0 => {
            "overview: no components yet; run `cartograph cluster` to group the code".to_string()
        }
        None => format!("overview: components {child_count}"),
    }
}

pub fn component_line(view: &ComponentView) -> String {
    let component = &view.component;
    let name = view
        .description
        .as_ref()
        .map_or(component.name.as_str(), |description| {
            description.name.as_str()
        });
    let mut line = format!("#{} {name}", component.id.0);
    if let Some(description) = &view.description {
        let _ = write!(line, "  {}", first_sentence(&description.summary));
        if !description.tags.is_empty() {
            let _ = write!(line, "  [{}]", description.tags.join(", "));
        }
    }
    if view.description_stale || component.stale {
        line.push_str("  (out of date)");
    }
    line
}

fn first_sentence(summary: &str) -> &str {
    match summary.find(SUMMARY_SENTENCE_END) {
        Some(end) => &summary[..=end],
        None => summary,
    }
}

/// Files per language and state, edges by kind, and how many calls resolved.
pub fn coverage_summary(inputs: &CoverageInputs, call_resolution: Option<f64>) -> String {
    let mut text = String::new();
    let total_files: u64 = inputs.files.iter().map(|count| count.count).sum();
    let _ = writeln!(text, "coverage: {total_files} files");
    let mut languages: Vec<(&str, Vec<String>)> = Vec::new();
    for count in &inputs.files {
        let language = count.language.as_deref().unwrap_or("(none)");
        let entry = format!("{} {}", count.status, count.count);
        match languages.last_mut() {
            Some((last, entries)) if *last == language => entries.push(entry),
            _ => languages.push((language, vec![entry])),
        }
    }
    for (language, entries) in languages {
        let _ = writeln!(text, "{language}: {}", entries.join(", "));
    }
    let _ = writeln!(text, "edges: {}", edge_summary(inputs));
    let resolution = call_resolution
        .map(|share| format!(", {:.1}% of calls resolved", share * 100.0))
        .unwrap_or_default();
    let _ = writeln!(
        text,
        "unresolved calls: {}{resolution}",
        inputs.unresolved_edges
    );
    let _ = write!(text, "not parsed: {} files", inputs.problem_file_total);
    text
}

fn edge_summary(inputs: &CoverageInputs) -> String {
    let mut kinds: Vec<(EdgeKind, Vec<String>, u64)> = Vec::new();
    for count in &inputs.edges {
        let entry = format!("{} {}", count.provenance, count.count);
        match kinds.last_mut() {
            Some((kind, entries, total)) if *kind == count.kind => {
                entries.push(entry);
                *total += count.count;
            }
            _ => kinds.push((count.kind, vec![entry], count.count)),
        }
    }
    let described: Vec<String> = kinds
        .iter()
        .map(|(kind, entries, total)| format!("{kind} {total} ({})", entries.join(", ")))
        .collect();
    if described.is_empty() {
        "none".to_string()
    } else {
        described.join("; ")
    }
}

pub fn problem_file_line(file: &graph::ProblemFile) -> String {
    match &file.detail {
        Some(detail) => format!("  {} {}: {detail}", file.path, file.status),
        None => format!("  {} {}", file.path, file.status),
    }
}
