use std::collections::HashMap;

use graph::{EdgeKind, NodeKind, Provenance};

use super::NAMESPACE_SEPARATOR;
use super::laravel;
use crate::model::{Extraction, Reference, ReferenceKind};
use crate::module::{Endpoint, ResolvedEdge, origin_endpoint};
use crate::project::{DefinitionRef, ProjectView};

/// How far base classes are followed before the lookup gives up.
pub(super) const MAX_HIERARCHY_DEPTH: u32 = 12;
const CONSTRUCTOR: &str = "__construct";
const FUNCTION_IMPORT: &str = "function";

pub(super) fn resolve(
    path: &str,
    extraction: &Extraction,
    project: &ProjectView<'_>,
) -> Vec<ResolvedEdge> {
    let resolver = Resolver::new(path, extraction, project);
    extraction
        .references
        .iter()
        .filter_map(|reference| match reference.kind {
            ReferenceKind::Framework(link) => laravel::resolve::edge(&resolver, reference, link),
            _ => resolver.edge(reference),
        })
        .collect()
}

/// What a class name led to.
pub(super) enum Lookup<'a> {
    Found(DefinitionRef<'a>),
    Ambiguous(usize),
    /// Not in the project: a vendor class or a built-in.
    External,
}

/// What a reference resolved to, before it becomes an edge from the reference's origin.
struct Outcome {
    kind: EdgeKind,
    dst: Endpoint,
    provenance: Provenance,
    reason: Option<String>,
}

impl Outcome {
    fn extracted(kind: EdgeKind, found: DefinitionRef<'_>) -> Self {
        Outcome {
            kind,
            dst: definition_endpoint(found),
            provenance: Provenance::Extracted,
            reason: None,
        }
    }

    fn inferred(kind: EdgeKind, found: DefinitionRef<'_>, reason: &str) -> Self {
        Outcome {
            kind,
            dst: definition_endpoint(found),
            provenance: Provenance::Inferred,
            reason: Some(reason.to_string()),
        }
    }

    fn unresolved(kind: EdgeKind, name: &str) -> Self {
        Outcome {
            kind,
            dst: Endpoint::Unresolved(name.to_string()),
            provenance: Provenance::Extracted,
            reason: Some("no definition found".to_string()),
        }
    }

    fn ambiguous(kind: EdgeKind, name: &str, count: usize) -> Self {
        Outcome {
            kind,
            dst: Endpoint::Unresolved(name.to_string()),
            provenance: Provenance::Extracted,
            reason: Some(format!("ambiguous: {count} candidates")),
        }
    }
}

pub(super) struct Resolver<'p, 'a> {
    pub(super) path: &'p str,
    pub(super) extraction: &'p Extraction,
    pub(super) project: &'p ProjectView<'a>,
    /// `use` bindings by namespace, then local name; the value is the imported name.
    class_imports: HashMap<&'p str, HashMap<&'p str, &'p str>>,
    function_imports: HashMap<&'p str, HashMap<&'p str, &'p str>>,
}

impl<'p, 'a> Resolver<'p, 'a> {
    pub(super) fn new(
        path: &'p str,
        extraction: &'p Extraction,
        project: &'p ProjectView<'a>,
    ) -> Self {
        let mut class_imports: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        let mut function_imports: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        for reference in &extraction.references {
            let (ReferenceKind::Use { .. }, Some(alias)) =
                (reference.kind, reference.alias.as_deref())
            else {
                continue;
            };
            let imports = if reference.member.as_deref() == Some(FUNCTION_IMPORT) {
                &mut function_imports
            } else {
                &mut class_imports
            };
            imports
                .entry(reference.scope.as_str())
                .or_default()
                .insert(alias, reference.text.as_str());
        }
        Resolver {
            path,
            extraction,
            project,
            class_imports,
            function_imports,
        }
    }

    fn edge(&self, reference: &Reference) -> Option<ResolvedEdge> {
        let outcome = match reference.kind {
            ReferenceKind::Use { .. } => self.import(reference)?,
            ReferenceKind::Call { method: false } => self.path_call(reference)?,
            ReferenceKind::Call { method: true } => self.method_call(reference)?,
            ReferenceKind::Inherits => self.type_edge(EdgeKind::Inherits, reference)?,
            ReferenceKind::Implements => self.type_edge(EdgeKind::Implements, reference)?,
            _ => return None,
        };
        Some(ResolvedEdge {
            src: origin_endpoint(self.path, self.extraction, reference.from),
            dst: outcome.dst,
            kind: outcome.kind,
            provenance: outcome.provenance,
            line: reference.line,
            reason: outcome.reason,
        })
    }

    fn import(&self, reference: &Reference) -> Option<Outcome> {
        let found = if reference.member.as_deref() == Some(FUNCTION_IMPORT) {
            self.find_definitions(&reference.text, &[NodeKind::Function])
        } else {
            self.find_class(&reference.text)
        };
        match found {
            Lookup::Found(target) if target.file != self.path => Some(Outcome {
                kind: EdgeKind::Imports,
                dst: Endpoint::File(target.file.to_string()),
                provenance: Provenance::Extracted,
                reason: None,
            }),
            Lookup::Ambiguous(count) => Some(Outcome::ambiguous(
                EdgeKind::Imports,
                &reference.text,
                count,
            )),
            _ => None,
        }
    }

    fn path_call(&self, reference: &Reference) -> Option<Outcome> {
        let Some((owner, member)) = reference.text.split_once("::") else {
            return self.function_call(reference);
        };
        let receiver = reference.receiver_type.as_deref();
        if owner == "parent" {
            let Lookup::Found(current) = self.class_named(receiver?) else {
                return None;
            };
            return self
                .parent_method(current, member)
                .map(|found| Outcome::extracted(EdgeKind::Calls, found));
        }
        match self.find_class(&self.class_name(&reference.scope, owner, receiver)) {
            Lookup::Found(class) => match self.method_in_hierarchy(class, member) {
                Some(found) => Some(Outcome::extracted(EdgeKind::Calls, found)),
                None if member == CONSTRUCTOR => Some(Outcome::extracted(EdgeKind::Calls, class)),
                None => None,
            },
            Lookup::Ambiguous(count) => {
                Some(Outcome::ambiguous(EdgeKind::Calls, &reference.text, count))
            }
            Lookup::External => None,
        }
    }

    fn function_call(&self, reference: &Reference) -> Option<Outcome> {
        let written = reference.text.as_str();
        let mut is_imported = false;
        let candidates = match written.strip_prefix(NAMESPACE_SEPARATOR) {
            Some(absolute) => vec![absolute.to_string()],
            None => {
                let imported = self
                    .function_imports
                    .get(reference.scope.as_str())
                    .and_then(|imports| imports.get(written));
                is_imported = imported.is_some();
                match imported {
                    Some(full) => vec![full.to_string()],
                    None => vec![
                        join_namespace(&reference.scope, written),
                        written.to_string(),
                    ],
                }
            }
        };
        for candidate in candidates {
            match self.find_definitions(&candidate, &[NodeKind::Function]) {
                Lookup::Found(found) => return Some(Outcome::extracted(EdgeKind::Calls, found)),
                Lookup::Ambiguous(count) => {
                    return Some(Outcome::ambiguous(EdgeKind::Calls, written, count));
                }
                Lookup::External => {}
            }
        }
        // An imported function lives in a vendor package; only unexplained names are counted.
        (!is_imported).then(|| Outcome::unresolved(EdgeKind::Calls, written))
    }

    fn method_call(&self, reference: &Reference) -> Option<Outcome> {
        let name = reference.text.as_str();
        let receiver = reference.receiver_type.as_deref();
        if let Some(receiver) = receiver {
            let class = if reference.via_self {
                self.class_named(receiver)
            } else {
                self.find_class(&self.class_name(&reference.scope, receiver, None))
            };
            return match class {
                Lookup::Found(class) => {
                    let reason = if reference.via_self {
                        "method of the enclosing class or its parents"
                    } else {
                        "method of the declared type of the receiver"
                    };
                    self.method_in_hierarchy(class, name)
                        .map(|found| Outcome::inferred(EdgeKind::Calls, found, reason))
                }
                Lookup::Ambiguous(count) => Some(Outcome::ambiguous(EdgeKind::Calls, name, count)),
                Lookup::External => None,
            };
        }
        let candidates: Vec<DefinitionRef<'a>> = self
            .project
            .named(name)
            .iter()
            .filter(|found| found.definition.kind == NodeKind::Method)
            .copied()
            .collect();
        match candidates.as_slice() {
            [] => None,
            [only] => Some(Outcome::inferred(
                EdgeKind::Calls,
                *only,
                "only method with that name",
            )),
            several => {
                let same_file: Vec<_> = several
                    .iter()
                    .filter(|found| found.file == self.path)
                    .collect();
                match same_file.as_slice() {
                    [only] => Some(Outcome::inferred(
                        EdgeKind::Calls,
                        **only,
                        "method in the same file",
                    )),
                    _ => Some(Outcome::ambiguous(EdgeKind::Calls, name, several.len())),
                }
            }
        }
    }

    fn type_edge(&self, kind: EdgeKind, reference: &Reference) -> Option<Outcome> {
        match self.find_class(&self.class_name(&reference.scope, &reference.text, None)) {
            Lookup::Found(found) => Some(Outcome::extracted(kind, found)),
            Lookup::Ambiguous(count) => Some(Outcome::ambiguous(kind, &reference.text, count)),
            Lookup::External => None,
        }
    }

    /// The fully qualified name (without a leading `\`) that `written` means in `scope`.
    /// `receiver` is the enclosing class, for `self` and `static`.
    pub(super) fn class_name(&self, scope: &str, written: &str, receiver: Option<&str>) -> String {
        if let Some(absolute) = written.strip_prefix(NAMESPACE_SEPARATOR) {
            return absolute.to_string();
        }
        if matches!(written, "self" | "static")
            && let Some(receiver) = receiver
        {
            return receiver.trim_start_matches(NAMESPACE_SEPARATOR).to_string();
        }
        let (first, rest) = match written.split_once(NAMESPACE_SEPARATOR) {
            Some((first, rest)) => (first, Some(rest)),
            None => (written, None),
        };
        match self
            .class_imports
            .get(scope)
            .and_then(|imports| imports.get(first))
        {
            Some(imported) => match rest {
                Some(rest) => format!("{imported}{NAMESPACE_SEPARATOR}{rest}"),
                None => imported.to_string(),
            },
            None => join_namespace(scope, written),
        }
    }

    pub(super) fn find_class(&self, fully_qualified: &str) -> Lookup<'a> {
        self.find_definitions(fully_qualified, &[NodeKind::Class, NodeKind::Type])
    }

    /// A class written as a `\`-prefixed name, as the extractor records the enclosing class.
    fn class_named(&self, written: &str) -> Lookup<'a> {
        self.find_class(written.trim_start_matches(NAMESPACE_SEPARATOR))
    }

    fn find_definitions(&self, fully_qualified: &str, kinds: &[NodeKind]) -> Lookup<'a> {
        let hits: Vec<_> = self
            .project
            .qualified(fully_qualified)
            .iter()
            .filter(|found| kinds.contains(&found.definition.kind))
            .collect();
        match hits.as_slice() {
            [] => Lookup::External,
            [only] => Lookup::Found(**only),
            several => Lookup::Ambiguous(several.len()),
        }
    }

    /// A method named `name` on `class` or, failing that, on its parents and traits.
    pub(super) fn method_in_hierarchy(
        &self,
        class: DefinitionRef<'a>,
        name: &str,
    ) -> Option<DefinitionRef<'a>> {
        method_in_hierarchy(self.project, class, name, 0)
    }

    fn parent_method(&self, class: DefinitionRef<'a>, name: &str) -> Option<DefinitionRef<'a>> {
        parents_of(self.project, class, true)
            .into_iter()
            .find_map(|parent| method_in_hierarchy(self.project, parent.found?, name, 1))
    }
}

/// A class named by `extends`, `implements` or a trait `use`.
pub(super) struct Parent<'a> {
    pub(super) name: String,
    pub(super) found: Option<DefinitionRef<'a>>,
}

/// The parents of `class`, resolved in the file that declares it.
pub(super) fn parents_of<'a>(
    project: &ProjectView<'a>,
    class: DefinitionRef<'a>,
    with_interfaces_and_traits: bool,
) -> Vec<Parent<'a>> {
    let Some(extraction) = project.extraction(class.file) else {
        return Vec::new();
    };
    let Some(class_index) = extraction
        .definitions
        .iter()
        .position(|definition| std::ptr::eq(definition, class.definition))
    else {
        return Vec::new();
    };
    let resolver = Resolver::new(class.file, extraction, project);
    extraction
        .references
        .iter()
        .filter(|reference| reference.from == Some(class_index))
        .filter(|reference| {
            reference.kind == ReferenceKind::Inherits
                || (with_interfaces_and_traits && reference.kind == ReferenceKind::Implements)
        })
        .map(|reference| {
            let name = resolver.class_name(&reference.scope, &reference.text, None);
            let found = match resolver.find_class(&name) {
                Lookup::Found(found) => Some(found),
                _ => None,
            };
            Parent { name, found }
        })
        .collect()
}

fn method_in_hierarchy<'a>(
    project: &ProjectView<'a>,
    class: DefinitionRef<'a>,
    name: &str,
    depth: u32,
) -> Option<DefinitionRef<'a>> {
    if depth > MAX_HIERARCHY_DEPTH {
        return None;
    }
    let own = format!("{}::{name}", class.definition.qualified_name);
    if let Some(found) = project
        .qualified(&own)
        .iter()
        .find(|found| found.file == class.file)
    {
        return Some(*found);
    }
    parents_of(project, class, true)
        .into_iter()
        .find_map(|parent| method_in_hierarchy(project, parent.found?, name, depth + 1))
}

fn join_namespace(namespace: &str, name: &str) -> String {
    match namespace {
        "" => name.to_string(),
        namespace => format!("{namespace}{NAMESPACE_SEPARATOR}{name}"),
    }
}

pub(super) fn definition_endpoint(found: DefinitionRef<'_>) -> Endpoint {
    Endpoint::Definition {
        file: found.file.to_string(),
        kind: found.definition.kind,
        qualified_name: found.definition.qualified_name.clone(),
    }
}
