use std::collections::HashSet;

use graph::{EdgeKind, NodeKind, Provenance};

use super::SOURCE_EXTENSIONS;
use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{Endpoint, ResolvedEdge, origin_endpoint};
use crate::paths::{file_stem, join, parent_dir};
use crate::project::{DefinitionRef, ProjectView};

const CALLABLE: [NodeKind; 2] = [NodeKind::Function, NodeKind::Method];
const INHERITABLE: [NodeKind; 2] = [NodeKind::Class, NodeKind::Type];

pub(super) fn resolve(
    path: &str,
    extraction: &Extraction,
    project: &ProjectView<'_>,
) -> Vec<ResolvedEdge> {
    let mut resolver = Resolver {
        path,
        project,
        visible: None,
    };
    extraction
        .references
        .iter()
        .filter_map(|reference| {
            let outcome = match reference.kind {
                ReferenceKind::Include { system } => resolver.include(reference, system)?,
                ReferenceKind::Call { method } => resolver.call(reference, method)?,
                ReferenceKind::Inherits => resolver.inherited(reference),
                _ => return None,
            };
            Some(outcome.into_edge(path, extraction, reference))
        })
        .collect()
}

/// What a reference resolved to, before it becomes an edge.
struct Outcome {
    kind: EdgeKind,
    dst: Endpoint,
    provenance: Provenance,
    reason: Option<String>,
}

impl Outcome {
    fn found(kind: EdgeKind, target: DefinitionRef<'_>, reason: &str) -> Self {
        Outcome {
            kind,
            dst: Endpoint::Definition {
                file: target.file.to_string(),
                kind: target.definition.kind,
                qualified_name: target.definition.qualified_name.clone(),
            },
            provenance: Provenance::Inferred,
            reason: Some(reason.to_string()),
        }
    }

    fn unresolved(kind: EdgeKind, name: &str, reason: String) -> Self {
        Outcome {
            kind,
            dst: Endpoint::Unresolved(name.to_string()),
            provenance: Provenance::Extracted,
            reason: Some(reason),
        }
    }

    fn into_edge(self, path: &str, extraction: &Extraction, reference: &Reference) -> ResolvedEdge {
        ResolvedEdge {
            src: origin_endpoint(path, extraction, reference.from),
            dst: self.dst,
            kind: self.kind,
            provenance: self.provenance,
            line: reference.line,
            reason: self.reason,
        }
    }
}

struct Resolver<'p, 'a> {
    path: &'p str,
    project: &'p ProjectView<'a>,
    /// Files whose definitions the current file can see through its includes; computed on first need.
    visible: Option<HashSet<String>>,
}

enum IncludeHit {
    Exact(String),
    /// Found by matching the end of the path, not by an include directory.
    Suffix(String),
    Ambiguous(usize),
    Missing,
}

impl<'a> Resolver<'_, 'a> {
    fn include(&self, reference: &Reference, system: bool) -> Option<Outcome> {
        let kind = EdgeKind::Imports;
        match locate_include(self.project, self.path, &reference.text, system) {
            IncludeHit::Exact(target) => Some(Outcome {
                kind,
                dst: Endpoint::File(target),
                provenance: Provenance::Extracted,
                reason: None,
            }),
            IncludeHit::Suffix(target) => Some(Outcome {
                kind,
                dst: Endpoint::File(target),
                provenance: Provenance::Inferred,
                reason: Some("include matched by path suffix".to_string()),
            }),
            IncludeHit::Ambiguous(count) => Some(Outcome::unresolved(
                kind,
                &reference.text,
                format!("ambiguous include: {count} files match"),
            )),
            // Angle-bracket headers that are not in the tree belong to the toolchain.
            IncludeHit::Missing if system => None,
            IncludeHit::Missing => Some(Outcome::unresolved(
                kind,
                &reference.text,
                "include not found in the tree".to_string(),
            )),
        }
    }

    fn call(&mut self, reference: &Reference, method: bool) -> Option<Outcome> {
        let kind = EdgeKind::Calls;
        let candidates = if method {
            self.members_named(&reference.text)
        } else {
            self.callable_candidates(reference)
        };
        if candidates.is_empty() {
            // A method on some expression with no definition here is almost always library code.
            let external = method || reference.text.starts_with("std::");
            return (!external).then(|| {
                Outcome::unresolved(kind, &reference.text, "no definition found".to_string())
            });
        }
        Some(self.choose(kind, reference, candidates))
    }

    fn inherited(&mut self, reference: &Reference) -> Outcome {
        let candidates = self.scoped_candidates(reference, &INHERITABLE);
        if candidates.is_empty() {
            let external = reference.text.starts_with("std::");
            let reason = if external {
                "base class outside the tree"
            } else {
                "no definition found"
            };
            return Outcome::unresolved(EdgeKind::Inherits, &reference.text, reason.to_string());
        }
        self.choose(EdgeKind::Inherits, reference, candidates)
    }

    fn members_named(&self, name: &str) -> Vec<DefinitionRef<'a>> {
        self.project
            .named(name)
            .iter()
            .filter(|found| found.definition.kind == NodeKind::Method)
            .copied()
            .collect()
    }

    /// Functions first; a call to a class name is a constructor call, so classes only count when
    /// no function matches.
    fn callable_candidates(&self, reference: &Reference) -> Vec<DefinitionRef<'a>> {
        let functions = self.scoped_candidates(reference, &CALLABLE);
        if !functions.is_empty() {
            return functions;
        }
        self.scoped_candidates(reference, &[NodeKind::Class])
    }

    /// Looks the name up from the innermost enclosing scope outward, then by suffix, then by
    /// last name segment alone.
    fn scoped_candidates(
        &self,
        reference: &Reference,
        kinds: &[NodeKind],
    ) -> Vec<DefinitionRef<'a>> {
        let wanted = |found: &&DefinitionRef<'a>| kinds.contains(&found.definition.kind);
        let mut scope = reference.scope.as_str();
        loop {
            let qualified = if scope.is_empty() {
                reference.text.clone()
            } else {
                format!("{scope}::{}", reference.text)
            };
            let hits: Vec<_> = self
                .project
                .qualified(&qualified)
                .iter()
                .filter(wanted)
                .copied()
                .collect();
            if !hits.is_empty() {
                return hits;
            }
            match scope.rsplit_once("::") {
                Some((outer, _)) => scope = outer,
                None if scope.is_empty() => break,
                None => scope = "",
            }
        }
        let last = reference
            .text
            .rsplit("::")
            .next()
            .unwrap_or(&reference.text);
        let suffix = format!("::{}", reference.text);
        let named = self.project.named(last).iter().filter(wanted).copied();
        if reference.text.contains("::") {
            return named
                .filter(|found| found.definition.qualified_name.ends_with(&suffix))
                .collect();
        }
        named.collect()
    }

    /// Narrows several candidates one clue at a time; a clue that would leave none is ignored.
    fn choose(
        &mut self,
        kind: EdgeKind,
        reference: &Reference,
        mut candidates: Vec<DefinitionRef<'a>>,
    ) -> Outcome {
        let mut reason = "only definition the name lookup found";
        if candidates.len() > 1 {
            let arity_matches = |found: &DefinitionRef<'a>| arity_fits(found.definition, reference);
            let path = self.path;
            let same_file = |found: &DefinitionRef<'a>| found.file == path;
            let same_scope = |found: &DefinitionRef<'a>| {
                super::extract::qualifier(&found.definition.qualified_name) == reference.scope
            };
            let visible = self.visible_files();
            let is_visible = |found: &DefinitionRef<'a>| visible.contains(found.file);
            let clues: [(&str, Clue<'_, 'a>); 4] = [
                ("narrowed by argument count", &arity_matches),
                ("narrowed to the same file", &same_file),
                ("narrowed to the same scope", &same_scope),
                ("narrowed to files visible through includes", &is_visible),
            ];
            for (clue_reason, clue) in clues {
                let kept: Vec<_> = candidates
                    .iter()
                    .filter(|found| clue(found))
                    .copied()
                    .collect();
                if !kept.is_empty() && kept.len() < candidates.len() {
                    candidates = kept;
                    reason = clue_reason;
                }
                if candidates.len() == 1 {
                    break;
                }
            }
        }
        match candidates.as_slice() {
            [only] => Outcome::found(kind, *only, reason),
            several => Outcome::unresolved(
                kind,
                &reference.text,
                format!("ambiguous: {} candidates", several.len()),
            ),
        }
    }

    fn visible_files(&mut self) -> &HashSet<String> {
        let (path, project) = (self.path, self.project);
        self.visible
            .get_or_insert_with(|| visible_through_includes(path, project))
    }
}

/// A test that keeps the candidates a clue favours.
type Clue<'c, 'a> = &'c dyn Fn(&DefinitionRef<'a>) -> bool;

fn arity_fits(definition: &Definition, reference: &Reference) -> bool {
    match (definition.arity, reference.arguments) {
        (Some(parameters), Some(arguments)) => parameters == arguments,
        _ => true,
    }
}

/// The file itself, everything it includes directly or not, and the source file that
/// implements each of those headers.
fn visible_through_includes(path: &str, project: &ProjectView<'_>) -> HashSet<String> {
    let mut seen: HashSet<String> = HashSet::from([path.to_string()]);
    let mut queue = vec![path.to_string()];
    while let Some(file) = queue.pop() {
        let Some(extraction) = project.extraction(&file) else {
            continue;
        };
        for reference in &extraction.references {
            let ReferenceKind::Include { system } = reference.kind else {
                continue;
            };
            match locate_include(project, &file, &reference.text, system) {
                IncludeHit::Exact(target) | IncludeHit::Suffix(target)
                    if seen.insert(target.clone()) =>
                {
                    queue.push(target);
                }
                _ => {}
            }
        }
    }
    let implementations: Vec<String> = seen
        .iter()
        .flat_map(|header| implementations_of(header, project))
        .collect();
    seen.extend(implementations);
    seen
}

/// Source files next to `header` with the same name, such as `foo.cpp` for `foo.h`.
fn implementations_of(header: &str, project: &ProjectView<'_>) -> Vec<String> {
    project
        .files_with_stem(file_stem(header))
        .iter()
        .filter(|candidate| {
            parent_dir(candidate) == parent_dir(header)
                && candidate
                    .rsplit_once('.')
                    .is_some_and(|(_, extension)| SOURCE_EXTENSIONS.contains(&extension))
        })
        .map(|candidate| candidate.to_string())
        .collect()
}

fn locate_include(project: &ProjectView<'_>, from: &str, text: &str, system: bool) -> IncludeHit {
    let beside_includer = (!system).then(|| join(parent_dir(from), text));
    let from_include_dirs = project
        .context
        .compile_commands
        .include_dirs_of(from)
        .iter()
        .map(|dir| join(dir, text));
    let exact = beside_includer
        .into_iter()
        .chain(from_include_dirs)
        .chain(std::iter::once(join("", text)))
        .find(|candidate| project.has_file(candidate));
    if let Some(target) = exact {
        return IncludeHit::Exact(target);
    }
    let wanted = join("", text);
    let suffix = format!("/{wanted}");
    let matches: Vec<&str> = project
        .files_with_stem(file_stem(&wanted))
        .iter()
        .copied()
        .filter(|candidate| candidate.ends_with(&suffix))
        .collect();
    match matches.as_slice() {
        [] => IncludeHit::Missing,
        [only] => IncludeHit::Suffix(only.to_string()),
        several => nearest(from, several)
            .map(|target| IncludeHit::Suffix(target.to_string()))
            .unwrap_or(IncludeHit::Ambiguous(several.len())),
    }
}

/// The candidate sharing the longest directory prefix with `from`, if one is strictly closest.
fn nearest<'a>(from: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let shared = |candidate: &str| {
        from.split('/')
            .zip(candidate.split('/'))
            .take_while(|(left, right)| left == right)
            .count()
    };
    let best = candidates.iter().map(|candidate| shared(candidate)).max()?;
    let mut closest = candidates
        .iter()
        .filter(|candidate| shared(candidate) == best);
    let first = closest.next()?;
    closest.next().is_none().then_some(*first)
}
