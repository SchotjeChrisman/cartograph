use std::collections::HashSet;

use graph::{EdgeKind, Provenance};
use languages::{DefinitionRef, Endpoint, ProjectView, ResolvedEdge};

use super::Link;

pub(super) struct DeclaredEdges {
    pub(super) edges: Vec<ResolvedEdge>,
    /// Ends that matched nothing or several things.
    pub(super) problems: Vec<String>,
}

pub(super) fn resolve(
    path: &str,
    links: &[Link],
    files: &HashSet<&str>,
    project: &ProjectView<'_>,
) -> DeclaredEdges {
    let mut declared = DeclaredEdges {
        edges: Vec::new(),
        problems: Vec::new(),
    };
    for link in links {
        let mut why_unresolved = Vec::new();
        let src = match &link.from {
            Some(name) => end(
                name,
                files,
                project,
                link.line,
                &mut declared.problems,
                &mut why_unresolved,
            ),
            None => Endpoint::File(path.to_string()),
        };
        let dst = end(
            &link.to,
            files,
            project,
            link.line,
            &mut declared.problems,
            &mut why_unresolved,
        );
        let label = match &link.note {
            Some(note) => format!("{}: {note}", link.kind),
            None => link.kind.clone(),
        };
        let reason = std::iter::once(label)
            .chain(why_unresolved)
            .collect::<Vec<_>>()
            .join("; ");
        declared.edges.push(ResolvedEdge {
            src,
            dst,
            kind: EdgeKind::Declared,
            provenance: Provenance::Declared,
            line: link.line,
            reason: Some(reason),
        });
    }
    declared
}

/// What `name` points at in the graph: a file path, else a qualified name, else a short name.
fn end(
    name: &str,
    files: &HashSet<&str>,
    project: &ProjectView<'_>,
    line: u32,
    problems: &mut Vec<String>,
    why_unresolved: &mut Vec<String>,
) -> Endpoint {
    if files.contains(name) {
        return Endpoint::File(name.to_string());
    }
    let matches = match project.qualified(name) {
        [] => project.named(name),
        qualified => qualified,
    };
    match matches {
        [only] => definition_endpoint(*only),
        [] => unresolved(
            name,
            "not found in the graph",
            line,
            problems,
            why_unresolved,
        ),
        several => unresolved(
            name,
            &format!("ambiguous: {} candidates", several.len()),
            line,
            problems,
            why_unresolved,
        ),
    }
}

fn unresolved(
    name: &str,
    why: &str,
    line: u32,
    problems: &mut Vec<String>,
    why_unresolved: &mut Vec<String>,
) -> Endpoint {
    problems.push(format!("line {line}: `{name}` {why}"));
    why_unresolved.push(format!("`{name}` {why}"));
    Endpoint::Unresolved(name.to_string())
}

fn definition_endpoint(found: DefinitionRef<'_>) -> Endpoint {
    Endpoint::Definition {
        file: found.file.to_string(),
        kind: found.definition.kind,
        qualified_name: found.definition.qualified_name.clone(),
    }
}
