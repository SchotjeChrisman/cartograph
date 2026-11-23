use std::collections::HashMap;

use graph::{EdgeKind, NodeKind, Provenance};

use super::SCRIPT_EXTENSIONS;
use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{Endpoint, ResolvedEdge, origin_endpoint};
use crate::paths::{join, normalise, parent_dir};
use crate::project::{DefinitionRef, ProjectView};

/// How far re-exports and base classes are followed before the lookup gives up.
const MAX_CHAIN_DEPTH: u32 = 8;
const ASSET_EXTENSIONS: [&str; 9] = [
    "css", "scss", "json", "svg", "png", "jpg", "gif", "html", "md",
];

pub(super) fn resolve(
    path: &str,
    extraction: &Extraction,
    project: &ProjectView<'_>,
) -> Vec<ResolvedEdge> {
    let resolver = Resolver::new(path, extraction, project);
    extraction
        .references
        .iter()
        .filter_map(|reference| {
            let outcome = match reference.kind {
                ReferenceKind::Import { .. } => resolver.import(reference)?,
                ReferenceKind::Call { method: false } => resolver.path_call(reference)?,
                ReferenceKind::Call { method: true } => resolver.method_call(reference)?,
                ReferenceKind::Inherits => resolver.type_edge(EdgeKind::Inherits, reference)?,
                ReferenceKind::Implements => resolver.type_edge(EdgeKind::Implements, reference)?,
                _ => return None,
            };
            Some(ResolvedEdge {
                src: origin_endpoint(path, extraction, reference.from),
                dst: outcome.dst,
                kind: outcome.kind,
                provenance: outcome.provenance,
                line: reference.line,
                reason: outcome.reason,
            })
        })
        .collect()
}

struct Outcome {
    kind: EdgeKind,
    dst: Endpoint,
    provenance: Provenance,
    reason: Option<String>,
}

impl Outcome {
    fn extracted(kind: EdgeKind, dst: Endpoint) -> Self {
        Outcome {
            kind,
            dst,
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

    fn unresolved(kind: EdgeKind, name: &str, reason: String) -> Self {
        Outcome {
            kind,
            dst: Endpoint::Unresolved(name.to_string()),
            provenance: Provenance::Extracted,
            reason: Some(reason),
        }
    }
}

/// What a file exports under a name.
#[derive(Clone, Copy)]
enum Export<'a> {
    Named(&'a str),
    Default,
}

struct Resolver<'p, 'a> {
    path: &'p str,
    project: &'p ProjectView<'a>,
    /// Imports by the local name they bind.
    bindings: HashMap<&'p str, &'p Reference>,
    by_qualified: HashMap<&'p str, &'p Definition>,
}

impl<'p, 'a> Resolver<'p, 'a> {
    fn new(path: &'p str, extraction: &'p Extraction, project: &'p ProjectView<'a>) -> Self {
        let bindings = extraction
            .references
            .iter()
            .filter(|reference| matches!(reference.kind, ReferenceKind::Import { reexport: false }))
            .filter_map(|reference| Some((reference.alias.as_deref()?, reference)))
            .collect();
        let by_qualified = extraction
            .definitions
            .iter()
            .map(|definition| (definition.qualified_name.as_str(), definition))
            .collect();
        Resolver {
            path,
            project,
            bindings,
            by_qualified,
        }
    }

    fn import(&self, reference: &Reference) -> Option<Outcome> {
        let specifier = reference.text.as_str();
        if !is_relative(specifier) {
            return None;
        }
        match resolve_specifier(self.project, self.path, specifier) {
            Some(target) if target == self.path => None,
            Some(target) => Some(Outcome::extracted(
                EdgeKind::Imports,
                Endpoint::File(target),
            )),
            None if has_asset_extension(specifier) => None,
            None => Some(Outcome::unresolved(
                EdgeKind::Imports,
                &join(parent_dir(self.path), specifier),
                "module file not found".to_string(),
            )),
        }
    }

    fn path_call(&self, reference: &Reference) -> Option<Outcome> {
        match self.path_target(
            &reference.scope,
            &reference.text,
            &[NodeKind::Function, NodeKind::Class],
        ) {
            Some(found) => Some(Outcome::extracted(
                EdgeKind::Calls,
                definition_endpoint(found),
            )),
            None => self
                .method_on_value(reference)
                .or_else(|| self.missing_bare_function(reference)),
        }
    }

    /// A bare name that no declaration or import explains. Names reach the program from
    /// outside only through imports and runtime globals, which carry a `::` once called.
    fn missing_bare_function(&self, reference: &Reference) -> Option<Outcome> {
        let name = reference.text.as_str();
        (!name.contains("::") && !self.bindings.contains_key(name))
            .then(|| Outcome::unresolved(EdgeKind::Calls, name, "no definition found".to_string()))
    }

    /// `value.member()` where `value` is not an import, class or function: a variable or
    /// parameter whose type is unknown, so the method is found by name alone.
    fn method_on_value(&self, reference: &Reference) -> Option<Outcome> {
        let (owner, member) = reference.text.split_once("::")?;
        let names_something = self.bindings.contains_key(owner)
            || self
                .lookup(
                    &reference.scope,
                    owner,
                    &[NodeKind::Function, NodeKind::Class],
                )
                .is_some();
        if names_something {
            return None;
        }
        self.method_by_name(member)
    }

    /// What a written name (`name` or `Owner::member`) points at.
    fn path_target(
        &self,
        scope: &str,
        text: &str,
        kinds: &[NodeKind],
    ) -> Option<DefinitionRef<'a>> {
        match text.split_once("::") {
            None => self.lookup(scope, text, kinds),
            Some((owner, member)) => self.member_of(scope, owner, member),
        }
    }

    /// `Owner.member`: a method of a class, or an export of a namespace import.
    fn member_of(&self, scope: &str, owner: &str, member: &str) -> Option<DefinitionRef<'a>> {
        if let Some(binding) = self
            .bindings
            .get(owner)
            .filter(|binding| binding.member.is_none())
        {
            let file = resolve_specifier(self.project, self.path, &binding.text)?;
            return exported(self.project, &file, Export::Named(member), 0);
        }
        let class = self.lookup(scope, owner, &[NodeKind::Class])?;
        method_of(self.project, class, member)
    }

    fn method_call(&self, reference: &Reference) -> Option<Outcome> {
        let name = reference.text.as_str();
        if reference.via_self
            && let Some(found) = self.method_of_receiver(reference, name)
        {
            return Some(Outcome::inferred(
                EdgeKind::Calls,
                found,
                "method of the enclosing class or its base",
            ));
        }
        self.method_by_name(name)
    }

    fn method_by_name(&self, name: &str) -> Option<Outcome> {
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
                    _ => Some(Outcome::unresolved(
                        EdgeKind::Calls,
                        name,
                        format!("ambiguous: {} candidates", several.len()),
                    )),
                }
            }
        }
    }

    /// A method found on the class around the call or on a base class.
    fn method_of_receiver(&self, reference: &Reference, name: &str) -> Option<DefinitionRef<'a>> {
        let class_name = reference.receiver_type.as_deref()?;
        let class = self
            .by_qualified
            .get(class_name)
            .and_then(|definition| self.own(definition))?;
        method_of(self.project, class, name)
    }

    fn type_edge(&self, kind: EdgeKind, reference: &Reference) -> Option<Outcome> {
        let found = self.path_target(
            &reference.scope,
            &reference.text,
            &[NodeKind::Class, NodeKind::Type],
        )?;
        Some(Outcome::extracted(kind, definition_endpoint(found)))
    }

    /// The definition a bare name means from `scope`: a declaration in an enclosing scope of
    /// this file, else something imported under that name. Members of a class are not in
    /// lexical scope, so methods are skipped while walking outward.
    fn lookup(&self, scope: &str, name: &str, kinds: &[NodeKind]) -> Option<DefinitionRef<'a>> {
        let mut scope = scope;
        loop {
            let qualified = if scope.is_empty() {
                name.to_string()
            } else {
                format!("{scope}::{name}")
            };
            let declared = self
                .by_qualified
                .get(qualified.as_str())
                .filter(|found| kinds.contains(&found.kind));
            if let Some(local) = declared.and_then(|definition| self.own(definition)) {
                return Some(local);
            }
            match scope.rsplit_once("::") {
                Some((outer, _)) => scope = outer,
                None if scope.is_empty() => break,
                None => scope = "",
            }
        }
        self.imported(name)
            .filter(|found| kinds.contains(&found.definition.kind))
    }

    /// The project's copy of one of this file's definitions, which carries the file path.
    fn own(&self, definition: &Definition) -> Option<DefinitionRef<'a>> {
        self.project
            .named(&definition.name)
            .iter()
            .find(|found| {
                found.file == self.path
                    && found.definition.qualified_name == definition.qualified_name
            })
            .copied()
    }

    fn imported(&self, local_name: &str) -> Option<DefinitionRef<'a>> {
        let binding = self.bindings.get(local_name)?;
        let file = resolve_specifier(self.project, self.path, &binding.text)?;
        let export = match binding.member.as_deref()? {
            "default" => Export::Default,
            member => Export::Named(member),
        };
        exported(self.project, &file, export, 0)
    }
}

/// The definition `file` exports, following re-exports and `export default name`.
fn exported<'a>(
    project: &ProjectView<'a>,
    file: &str,
    export: Export<'_>,
    depth: u32,
) -> Option<DefinitionRef<'a>> {
    if depth > MAX_CHAIN_DEPTH {
        return None;
    }
    let extraction = project.extraction(file)?;
    let name = match export {
        Export::Named(name) => name,
        Export::Default => extraction
            .references
            .iter()
            .find(|reference| reference.kind == ReferenceKind::DefaultExport)
            .map(|reference| reference.text.as_str())?,
    };
    if let Some(found) = project
        .qualified(name)
        .iter()
        .find(|found| found.file == file)
    {
        return Some(*found);
    }
    extraction
        .references
        .iter()
        .filter(|reference| reference.kind == (ReferenceKind::Import { reexport: true }))
        .find_map(|reexport| {
            let exported_name = reexport.alias.as_deref();
            let wanted = match (exported_name, &reexport.member) {
                (Some(alias), Some(original)) if alias == name => original.as_str(),
                (None, _) => name,
                _ => return None,
            };
            let source = resolve_specifier(project, file, &reexport.text)?;
            exported(project, &source, Export::Named(wanted), depth + 1)
        })
}

/// A method named `name` on `class` or, failing that, on the classes it extends.
fn method_of<'a>(
    project: &ProjectView<'a>,
    class: DefinitionRef<'a>,
    name: &str,
) -> Option<DefinitionRef<'a>> {
    method_of_at_depth(project, class, name, 0)
}

fn method_of_at_depth<'a>(
    project: &ProjectView<'a>,
    class: DefinitionRef<'a>,
    name: &str,
    depth: u32,
) -> Option<DefinitionRef<'a>> {
    if depth > MAX_CHAIN_DEPTH {
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
    let extraction = project.extraction(class.file)?;
    let class_index = extraction
        .definitions
        .iter()
        .position(|definition| std::ptr::eq(definition, class.definition))?;
    let resolver = Resolver::new(class.file, extraction, project);
    extraction
        .references
        .iter()
        .filter(|reference| {
            reference.kind == ReferenceKind::Inherits && reference.from == Some(class_index)
        })
        .find_map(|base| {
            let base_class = resolver.path_target(&base.scope, &base.text, &[NodeKind::Class])?;
            method_of_at_depth(project, base_class, name, depth + 1)
        })
}

/// The file a module specifier names, tried the way bundlers and `tsc` do.
fn resolve_specifier(
    project: &ProjectView<'_>,
    from_file: &str,
    specifier: &str,
) -> Option<String> {
    if !is_relative(specifier) {
        return None;
    }
    let base = join(parent_dir(from_file), specifier);
    let sibling = |extension: &str| format!("{base}.{extension}");
    let index = |extension: &str| format!("{}/index.{extension}", base.trim_end_matches('/'));
    let transpiled_source = base
        .rsplit_once('.')
        .filter(|(_, extension)| matches!(*extension, "js" | "jsx" | "mjs" | "cjs"))
        .map(|(stem, extension)| format!("{stem}.{}", source_extension_of(extension)));
    std::iter::once(base.clone())
        .chain(transpiled_source)
        .chain(SCRIPT_EXTENSIONS.iter().map(|extension| sibling(extension)))
        .chain(SCRIPT_EXTENSIONS.iter().map(|extension| index(extension)))
        .map(|candidate| normalise(&candidate))
        .find(|candidate| project.has_file(candidate))
}

/// TypeScript writes imports with the extension the compiled file will have.
fn source_extension_of(compiled: &str) -> &'static str {
    match compiled {
        "jsx" => "tsx",
        "mjs" => "mts",
        "cjs" => "cts",
        _ => "ts",
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier == "."
        || specifier == ".."
}

fn has_asset_extension(specifier: &str) -> bool {
    specifier
        .rsplit_once('.')
        .is_some_and(|(_, extension)| ASSET_EXTENSIONS.contains(&extension))
}

fn definition_endpoint(found: DefinitionRef<'_>) -> Endpoint {
    Endpoint::Definition {
        file: found.file.to_string(),
        kind: found.definition.kind,
        qualified_name: found.definition.qualified_name.clone(),
    }
}
