use std::collections::HashMap;

use graph::{EdgeKind, NodeKind, Provenance};

use crate::model::{Extraction, Reference, ReferenceKind};
use crate::module::{Endpoint, ResolvedEdge, origin_endpoint};
use crate::project::{DefinitionRef, ProjectView};

/// How deep an alias may expand into other aliases before the lookup gives up.
const MAX_ALIAS_DEPTH: u32 = 8;
const CALLABLE: [NodeKind; 2] = [NodeKind::Function, NodeKind::Method];
/// Bare names the prelude brings into every module; a call to one is not a missing definition.
const PRELUDE_CALLABLES: [&str; 4] = ["Some", "Ok", "Err", "drop"];

pub(super) fn resolve(
    path: &str,
    extraction: &Extraction,
    project: &ProjectView<'_>,
) -> Vec<ResolvedEdge> {
    let resolver = Resolver::new(path, extraction, project);
    extraction
        .references
        .iter()
        .filter_map(|reference| resolver.edge_for(reference))
        .collect()
}

/// What a path lookup found.
enum Lookup<'a> {
    Found(Vec<DefinitionRef<'a>>),
    /// The path points into this project but nothing is there.
    Dangling,
    /// The path leaves the project: std, a dependency, a prelude name or an enum variant.
    External,
}

struct Binding<'a> {
    scope: &'a str,
    path: &'a str,
}

struct Resolver<'p, 'a> {
    path: &'p str,
    extraction: &'p Extraction,
    project: &'p ProjectView<'a>,
    /// `use` names visible per module scope.
    aliases: HashMap<&'p str, HashMap<&'p str, Binding<'p>>>,
    globs: Vec<Binding<'p>>,
}

impl<'p, 'a> Resolver<'p, 'a> {
    fn new(path: &'p str, extraction: &'p Extraction, project: &'p ProjectView<'a>) -> Self {
        let mut aliases: HashMap<&str, HashMap<&str, Binding<'_>>> = HashMap::new();
        let mut globs = Vec::new();
        for reference in &extraction.references {
            let ReferenceKind::Use { glob } = reference.kind else {
                continue;
            };
            let binding = Binding {
                scope: &reference.scope,
                path: &reference.text,
            };
            match (glob, reference.alias.as_deref()) {
                (true, _) => globs.push(binding),
                (false, Some(alias)) => {
                    aliases
                        .entry(reference.scope.as_str())
                        .or_default()
                        .insert(alias, binding);
                }
                (false, None) => {}
            }
        }
        Resolver {
            path,
            extraction,
            project,
            aliases,
            globs,
        }
    }

    fn edge_for(&self, reference: &Reference) -> Option<ResolvedEdge> {
        let (kind, src, dst, provenance, reason) = match reference.kind {
            ReferenceKind::Use { .. } => self.import(reference)?,
            ReferenceKind::ModuleDeclaration => self.module_declaration(reference)?,
            ReferenceKind::Call { method: true } => self.method_call(reference)?,
            ReferenceKind::Call { method: false } => self.path_call(reference)?,
            ReferenceKind::Inherits => self.inherits(reference)?,
            ReferenceKind::Implements => self.implements(reference)?,
            _ => return None,
        };
        Some(ResolvedEdge {
            src,
            dst,
            kind,
            provenance,
            line: reference.line,
            reason,
        })
    }

    fn origin(&self, reference: &Reference) -> Endpoint {
        origin_endpoint(self.path, self.extraction, reference.from)
    }

    fn import(&self, reference: &Reference) -> Option<Edge> {
        let src = Endpoint::File(self.path.to_string());
        match self.lookup(&reference.scope, None, &reference.text, 0) {
            Lookup::Found(hits) => {
                let file = hits.first()?.file;
                (file != self.path).then(|| {
                    (
                        EdgeKind::Imports,
                        src,
                        Endpoint::File(file.to_string()),
                        Provenance::Extracted,
                        None,
                    )
                })
            }
            Lookup::Dangling => Some(unresolved(
                EdgeKind::Imports,
                src,
                &reference.text,
                "path does not resolve",
            )),
            Lookup::External => None,
        }
    }

    fn module_declaration(&self, reference: &Reference) -> Option<Edge> {
        let qualified = format!("{}::{}", reference.scope, reference.text);
        let src = self.enclosing_module(&reference.scope);
        match self.project.qualified(&qualified) {
            [] => Some(unresolved(
                EdgeKind::Contains,
                src,
                &qualified,
                "module file not found",
            )),
            [only, ..] => Some((
                EdgeKind::Contains,
                src,
                definition_endpoint(*only),
                Provenance::Extracted,
                None,
            )),
        }
    }

    fn enclosing_module(&self, scope: &str) -> Endpoint {
        match self.extraction.definitions.iter().position(|definition| {
            definition.kind == NodeKind::Module && definition.qualified_name == scope
        }) {
            Some(index) => origin_endpoint(self.path, self.extraction, Some(index)),
            None => Endpoint::File(self.path.to_string()),
        }
    }

    fn path_call(&self, reference: &Reference) -> Option<Edge> {
        let src = self.origin(reference);
        let receiver = reference.receiver_type.as_deref();
        match self.lookup(&reference.scope, receiver, &reference.text, 0) {
            Lookup::Found(hits) => {
                let callable: Vec<_> = hits
                    .into_iter()
                    .filter(|hit| CALLABLE.contains(&hit.definition.kind))
                    .collect();
                match callable.as_slice() {
                    [] => None,
                    [only] => Some((
                        EdgeKind::Calls,
                        src,
                        definition_endpoint(*only),
                        Provenance::Extracted,
                        None,
                    )),
                    several => Some(ambiguous(
                        EdgeKind::Calls,
                        src,
                        &reference.text,
                        several.len(),
                    )),
                }
            }
            Lookup::Dangling => Some(unresolved(
                EdgeKind::Calls,
                src,
                &reference.text,
                "path does not resolve",
            )),
            Lookup::External => self.missing_bare_function(reference, src),
        }
    }

    /// A bare name that no `use`, module or prelude explains is a function the tree lacks;
    /// qualified paths may leave for std or a dependency, so they stay out of the count.
    fn missing_bare_function(&self, reference: &Reference, src: Endpoint) -> Option<Edge> {
        let name = reference.text.as_str();
        let is_bare = !name.contains("::");
        let is_known =
            PRELUDE_CALLABLES.contains(&name) || self.alias_for(&reference.scope, name).is_some();
        (is_bare && !is_known)
            .then(|| unresolved(EdgeKind::Calls, src, name, "no definition found"))
    }

    fn method_call(&self, reference: &Reference) -> Option<Edge> {
        let src = self.origin(reference);
        let mut candidates: Vec<DefinitionRef<'a>> = self
            .project
            .named(&reference.text)
            .iter()
            .filter(|found| found.definition.kind == NodeKind::Method)
            .copied()
            .collect();
        if candidates.is_empty() {
            // Methods of std and dependencies are not in the tree.
            return None;
        }
        let mut reason = "only method with that name";
        if candidates.len() > 1 {
            let owner = reference
                .receiver_type
                .as_deref()
                .filter(|_| reference.via_self);
            let narrowing: [(&str, Clue<'_, 'a>); 3] = [
                (
                    "method of the type behind self",
                    Box::new(|found| Some(owner_of(found)) == owner),
                ),
                (
                    "method in the same file",
                    Box::new(|found| found.file == self.path),
                ),
                (
                    "method in the same crate",
                    Box::new(|found| {
                        crate_of(&found.definition.qualified_name) == crate_of(&reference.scope)
                    }),
                ),
            ];
            for (clue_reason, clue) in narrowing {
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
            [only] => Some((
                EdgeKind::Calls,
                src,
                definition_endpoint(*only),
                Provenance::Inferred,
                Some(reason.to_string()),
            )),
            several => Some(ambiguous(
                EdgeKind::Calls,
                src,
                &reference.text,
                several.len(),
            )),
        }
    }

    fn inherits(&self, reference: &Reference) -> Option<Edge> {
        let src = self.origin(reference);
        self.type_edge(EdgeKind::Inherits, src, reference)
    }

    fn implements(&self, reference: &Reference) -> Option<Edge> {
        let type_name = reference.receiver_type.as_deref()?;
        let implementer = self.find_type(&reference.scope, type_name)?;
        self.type_edge(
            EdgeKind::Implements,
            definition_endpoint(implementer),
            reference,
        )
    }

    fn type_edge(&self, kind: EdgeKind, src: Endpoint, reference: &Reference) -> Option<Edge> {
        match self.lookup(&reference.scope, None, &reference.text, 0) {
            Lookup::Found(hits) => {
                let target = hits
                    .iter()
                    .find(|hit| matches!(hit.definition.kind, NodeKind::Type | NodeKind::Class))?;
                Some((
                    kind,
                    src,
                    definition_endpoint(*target),
                    Provenance::Extracted,
                    None,
                ))
            }
            Lookup::Dangling => Some(unresolved(
                kind,
                src,
                &reference.text,
                "path does not resolve",
            )),
            Lookup::External => None,
        }
    }

    /// The type an `impl` block is for: the guess made from the block's own module first, then
    /// whatever the bare name means through this file's `use` lines.
    fn find_type(&self, scope: &str, guessed_qualified_name: &str) -> Option<DefinitionRef<'a>> {
        let is_type = |found: &&DefinitionRef<'a>| {
            matches!(found.definition.kind, NodeKind::Type | NodeKind::Class)
        };
        if let Some(found) = self
            .project
            .qualified(guessed_qualified_name)
            .iter()
            .find(is_type)
        {
            return Some(*found);
        }
        let bare = guessed_qualified_name.rsplit("::").next()?;
        match self.lookup(scope, None, bare, 0) {
            Lookup::Found(hits) => hits.into_iter().find(|found| is_type(&found)),
            _ => None,
        }
    }

    fn lookup(
        &self,
        scope: &str,
        receiver_type: Option<&str>,
        text: &str,
        depth: u32,
    ) -> Lookup<'a> {
        if depth > MAX_ALIAS_DEPTH {
            return Lookup::External;
        }
        let mut segments = text.split("::");
        let first = segments.next().unwrap_or_default();
        let rest: Vec<&str> = segments.collect();
        let with_rest = |base: &str| join_qualified(base, &rest);
        match first {
            "crate" => self.exact(&with_rest(crate_of(scope)), true, depth),
            "self" => self.exact(&with_rest(scope), true, depth),
            "super" => {
                let mut base = scope;
                let mut remaining = rest.as_slice();
                base = parent_module(base);
                while let Some((&"super", after)) = remaining.split_first() {
                    base = parent_module(base);
                    remaining = after;
                }
                self.exact(&join_qualified(base, remaining), true, depth)
            }
            "Self" => match receiver_type {
                Some(receiver) => self.exact(&with_rest(receiver), true, depth),
                None => Lookup::External,
            },
            _ => self.lookup_unanchored(scope, receiver_type, first, &rest, depth),
        }
    }

    fn lookup_unanchored(
        &self,
        scope: &str,
        receiver_type: Option<&str>,
        first: &str,
        rest: &[&str],
        depth: u32,
    ) -> Lookup<'a> {
        if let Some(binding) = self.alias_for(scope, first) {
            let expanded = join_qualified(binding.path, rest);
            return self.lookup(binding.scope, receiver_type, &expanded, depth + 1);
        }
        let as_written = join_qualified(first, rest);
        if self.is_project_crate(first) {
            return self.exact(&as_written, true, depth);
        }
        let local = format!("{scope}::{as_written}");
        if let Lookup::Found(hits) = self.exact(&local, false, depth) {
            return Lookup::Found(hits);
        }
        for glob in &self.globs {
            let module = match self.lookup(glob.scope, None, glob.path, depth + 1) {
                Lookup::Found(hits) => hits
                    .first()
                    .map(|hit| hit.definition.qualified_name.clone()),
                _ => None,
            };
            if let Some(module) = module
                && let Lookup::Found(hits) = self.exact(&format!("{module}::{as_written}"), false, depth)
            {
                return Lookup::Found(hits);
            }
        }
        Lookup::External
    }

    /// An alias bound in `scope` or any module around it.
    fn alias_for(&self, scope: &str, name: &str) -> Option<&Binding<'p>> {
        let mut scope = scope;
        loop {
            if let Some(binding) = self
                .aliases
                .get(scope)
                .and_then(|bindings| bindings.get(name))
            {
                return Some(binding);
            }
            if !scope.contains("::") {
                return None;
            }
            scope = parent_module(scope);
        }
    }

    fn is_project_crate(&self, name: &str) -> bool {
        self.project
            .context
            .rust_crates
            .iter()
            .any(|package| package.name == name)
    }

    /// Looks up a fully qualified name. A miss under `internal` is first tried as a re-export
    /// (`pub use`) of the module that should hold it, then is a dangling path unless its parent
    /// is a type, which means an enum variant or associated item the index does not model.
    fn exact(&self, qualified_name: &str, internal: bool, depth: u32) -> Lookup<'a> {
        let hits = self.project.qualified(qualified_name);
        if !hits.is_empty() {
            return Lookup::Found(hits.to_vec());
        }
        if internal && let Some(found) = self.reexported(qualified_name, depth) {
            return found;
        }
        let parent_is_type = qualified_name.rsplit_once("::").is_some_and(|(parent, _)| {
            self.project
                .qualified(parent)
                .iter()
                .any(|found| matches!(found.definition.kind, NodeKind::Class | NodeKind::Type))
        });
        if internal && !parent_is_type {
            Lookup::Dangling
        } else {
            Lookup::External
        }
    }

    /// What `module::name` means when `module` does not define `name` but imports it, usually
    /// through `pub use`. Resolved the way the module's own file would resolve the bare name.
    fn reexported(&self, qualified_name: &str, depth: u32) -> Option<Lookup<'a>> {
        if depth > MAX_ALIAS_DEPTH {
            return None;
        }
        let (module, name) = qualified_name.rsplit_once("::")?;
        let module_file = self
            .project
            .qualified(module)
            .iter()
            .find(|found| found.definition.kind == NodeKind::Module)?
            .file;
        let extraction = self.project.extraction(module_file)?;
        let owner = Resolver::new(module_file, extraction, self.project);
        match owner.lookup_unanchored(module, None, name, &[], depth + 1) {
            Lookup::Found(hits) => Some(Lookup::Found(hits)),
            _ => None,
        }
    }
}

/// A test that keeps the candidates a clue favours.
type Clue<'c, 'a> = Box<dyn Fn(&DefinitionRef<'a>) -> bool + 'c>;

type Edge = (EdgeKind, Endpoint, Endpoint, Provenance, Option<String>);

fn unresolved(kind: EdgeKind, src: Endpoint, name: &str, reason: &str) -> Edge {
    (
        kind,
        src,
        Endpoint::Unresolved(name.to_string()),
        Provenance::Extracted,
        Some(reason.to_string()),
    )
}

fn ambiguous(kind: EdgeKind, src: Endpoint, name: &str, count: usize) -> Edge {
    unresolved(kind, src, name, &format!("ambiguous: {count} candidates"))
}

fn definition_endpoint(found: DefinitionRef<'_>) -> Endpoint {
    Endpoint::Definition {
        file: found.file.to_string(),
        kind: found.definition.kind,
        qualified_name: found.definition.qualified_name.clone(),
    }
}

fn join_qualified(base: &str, rest: &[&str]) -> String {
    std::iter::once(base)
        .chain(rest.iter().copied())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}

fn crate_of(qualified_name: &str) -> &str {
    qualified_name.split("::").next().unwrap_or_default()
}

fn parent_module(module: &str) -> &str {
    module
        .rsplit_once("::")
        .map_or(module, |(parent, _)| parent)
}

/// The type a method belongs to: `a::Type::method` gives `a::Type`.
fn owner_of<'d>(found: &DefinitionRef<'d>) -> &'d str {
    let qualified = found.definition.qualified_name.as_str();
    qualified.rsplit_once("::").map_or("", |(owner, _)| owner)
}
