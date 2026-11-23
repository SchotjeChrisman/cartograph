//! Turns the plugin's references into edges: handlers to routes, models to tables, listeners to
//! hooks, bindings to implementations, views to files, config keys and tables.

use graph::{EdgeKind, NodeKind, Provenance};

use super::inflect::conventional_table;
use crate::model::{FrameworkLink, Reference, ReferenceKind};
use crate::module::{Endpoint, ResolvedEdge, origin_endpoint};
use crate::php::resolve::{Lookup, MAX_HIERARCHY_DEPTH, Resolver, definition_endpoint, parents_of};
use crate::project::DefinitionRef;

const MODEL_BASES: [&str; 3] = [
    "Illuminate\\Database\\Eloquent\\Model",
    "Illuminate\\Foundation\\Auth\\User",
    "Illuminate\\Database\\Eloquent\\Relations\\Pivot",
];
const LEGACY_CONTROLLER_NAMESPACE: &str = "App\\Http\\Controllers";
const HANDLER_METHODS: [&str; 2] = ["handle", "__invoke"];
const VIEW_DIRECTORY: &str = "resources/views/";
const TEMPLATE_STEM_SUFFIX: &str = ".blade";
const PACKAGE_VIEW_SEPARATOR: &str = "::";
const COMPONENT_VIEW_PREFIX: &str = "components.";

pub(in crate::php) fn edge(
    resolver: &Resolver<'_, '_>,
    reference: &Reference,
    link: FrameworkLink,
) -> Option<ResolvedEdge> {
    let origin = origin_endpoint(resolver.path, resolver.extraction, reference.from);
    let at = |src: Endpoint,
              dst: Endpoint,
              kind: EdgeKind,
              provenance: Provenance,
              reason: Option<String>| ResolvedEdge {
        src,
        dst,
        kind,
        provenance,
        line: reference.line,
        reason,
    };
    let global = |kind: NodeKind, name: &str| Endpoint::Definition {
        file: String::new(),
        kind,
        qualified_name: name.to_string(),
    };
    match link {
        FrameworkLink::TableAccess { write } => {
            let kind = if write {
                EdgeKind::Writes
            } else {
                EdgeKind::Reads
            };
            Some(at(
                origin,
                global(NodeKind::Table, &reference.text),
                kind,
                Provenance::Extracted,
                None,
            ))
        }
        FrameworkLink::ConfigAccess { write } => {
            let kind = if write {
                EdgeKind::Writes
            } else {
                EdgeKind::Reads
            };
            Some(at(
                origin,
                global(NodeKind::ConfigKey, &reference.text),
                kind,
                Provenance::Extracted,
                None,
            ))
        }
        FrameworkLink::HookFired => {
            let hook = global(
                NodeKind::Hook,
                &resolver.class_name(&reference.scope, &reference.text, None),
            );
            Some(at(
                origin,
                hook,
                EdgeKind::FiresHook,
                Provenance::Extracted,
                None,
            ))
        }
        FrameworkLink::HookHandled => {
            let hook = global(
                NodeKind::Hook,
                &resolver.class_name(&reference.scope, &reference.text, None),
            );
            Some(at(
                origin,
                hook,
                EdgeKind::RegistersHook,
                Provenance::Extracted,
                None,
            ))
        }
        FrameworkLink::ModelTable => {
            model_table(resolver, reference).map(|(table, provenance, reason)| {
                at(
                    origin,
                    global(NodeKind::Table, &table),
                    EdgeKind::Reads,
                    provenance,
                    reason,
                )
            })
        }
        FrameworkLink::RouteHandler { optional } => {
            let (src, provenance, reason) = route_handler(resolver, reference, optional)?;
            Some(at(src, origin, EdgeKind::HandlesRoute, provenance, reason))
        }
        FrameworkLink::Listener => {
            let event = resolver.class_name(&reference.scope, reference.member.as_deref()?, None);
            let (src, reason) = listener_method(resolver, reference);
            let provenance = Provenance::Extracted;
            Some(at(
                src,
                global(NodeKind::Hook, &event),
                EdgeKind::RegistersHook,
                provenance,
                reason,
            ))
        }
        FrameworkLink::Binding => binding(resolver, reference).map(|(concrete, contract)| {
            let reason = Some("container binding".to_string());
            at(
                definition_endpoint(concrete),
                definition_endpoint(contract),
                EdgeKind::Implements,
                Provenance::Inferred,
                reason,
            )
        }),
        FrameworkLink::View => view(resolver, reference).map(|(dst, reason)| {
            at(
                origin,
                dst,
                EdgeKind::Imports,
                Provenance::Extracted,
                reason,
            )
        }),
    }
}

/// The table a model class uses, when the class really is an Eloquent model. An explicit
/// `$table` wins over the conventional name.
fn model_table(
    resolver: &Resolver<'_, '_>,
    reference: &Reference,
) -> Option<(String, Provenance, Option<String>)> {
    let class = resolver.extraction.definitions.get(reference.from?)?;
    let has_explicit_table = |candidate: &&Reference| {
        candidate.from == reference.from
            && candidate.kind == ReferenceKind::Framework(FrameworkLink::ModelTable)
            && !candidate.text.is_empty()
    };
    if reference.text.is_empty()
        && resolver
            .extraction
            .references
            .iter()
            .any(|candidate| has_explicit_table(&candidate))
    {
        return None;
    }
    let Lookup::Found(class_ref) = resolver.find_class(&class.qualified_name) else {
        return None;
    };
    if !is_model(resolver, class_ref, 0) {
        return None;
    }
    if reference.text.is_empty() {
        Some((
            conventional_table(&class.name),
            Provenance::Inferred,
            Some("conventional table name of the model".to_string()),
        ))
    } else {
        Some((reference.text.clone(), Provenance::Extracted, None))
    }
}

fn is_model(resolver: &Resolver<'_, '_>, class: DefinitionRef<'_>, depth: u32) -> bool {
    if depth > MAX_HIERARCHY_DEPTH {
        return false;
    }
    parents_of(resolver.project, class, false)
        .into_iter()
        .any(|parent| {
            MODEL_BASES.contains(&parent.name.as_str())
                || parent
                    .found
                    .is_some_and(|found| is_model(resolver, found, depth + 1))
        })
}

/// The handler method of a route, or an unresolved end that says what was missing.
fn route_handler(
    resolver: &Resolver<'_, '_>,
    reference: &Reference,
    optional: bool,
) -> Option<(Endpoint, Provenance, Option<String>)> {
    let action = reference.member.as_deref()?;
    let written = resolver.class_name(&reference.scope, &reference.text, None);
    let mut provenance = Provenance::Extracted;
    let mut reason = None;
    let mut found = resolver.find_class(&written);
    let mut class_name = written;
    if matches!(found, Lookup::External) && !reference.text.contains('\\') {
        let legacy = format!("{LEGACY_CONTROLLER_NAMESPACE}\\{}", reference.text);
        if let Lookup::Found(_) = resolver.find_class(&legacy) {
            found = resolver.find_class(&legacy);
            class_name = legacy;
            provenance = Provenance::Inferred;
            reason =
                Some("legacy controller string in the default controller namespace".to_string());
        }
    }
    match found {
        Lookup::Found(class) => match resolver.method_in_hierarchy(class, action) {
            Some(method) => Some((definition_endpoint(method), provenance, reason)),
            None if optional => None,
            None => Some(unresolved_handler(
                &format!("{class_name}::{action}"),
                "controller action not found",
            )),
        },
        Lookup::Ambiguous(count) => Some(unresolved_handler(
            &class_name,
            &format!("ambiguous: {count} candidates"),
        )),
        Lookup::External => Some(unresolved_handler(
            &class_name,
            "controller class not found",
        )),
    }
}

fn unresolved_handler(name: &str, reason: &str) -> (Endpoint, Provenance, Option<String>) {
    (
        Endpoint::Unresolved(name.to_string()),
        Provenance::Extracted,
        Some(reason.to_string()),
    )
}

/// The method that runs for a registered listener, or an unresolved end that says what was missing.
fn listener_method(
    resolver: &Resolver<'_, '_>,
    reference: &Reference,
) -> (Endpoint, Option<String>) {
    let class_name = resolver.class_name(&reference.scope, &reference.text, None);
    match resolver.find_class(&class_name) {
        Lookup::Found(class) => match HANDLER_METHODS
            .iter()
            .find_map(|method| resolver.method_in_hierarchy(class, method))
        {
            Some(method) => (definition_endpoint(method), None),
            None => (
                Endpoint::Unresolved(format!("{class_name}::handle")),
                Some("listener has no handle or __invoke method".to_string()),
            ),
        },
        Lookup::Ambiguous(count) => (
            Endpoint::Unresolved(class_name),
            Some(format!("ambiguous: {count} candidates")),
        ),
        Lookup::External => (
            Endpoint::Unresolved(class_name),
            Some("listener class not found".to_string()),
        ),
    }
}

/// The implementation and the contract of a binding, when both are in the project.
fn binding<'a>(
    resolver: &Resolver<'_, 'a>,
    reference: &Reference,
) -> Option<(DefinitionRef<'a>, DefinitionRef<'a>)> {
    let concrete = resolver.class_name(&reference.scope, reference.member.as_deref()?, None);
    let contract = resolver.class_name(&reference.scope, &reference.text, None);
    match (
        resolver.find_class(&concrete),
        resolver.find_class(&contract),
    ) {
        (Lookup::Found(concrete), Lookup::Found(contract)) => Some((concrete, contract)),
        _ => None,
    }
}

/// The template file a view name points at. Of several matches the one nearest to the
/// referring file wins; a tie stays unresolved.
fn view(resolver: &Resolver<'_, '_>, reference: &Reference) -> Option<(Endpoint, Option<String>)> {
    let name = reference.text.as_str();
    if name.contains(PACKAGE_VIEW_SEPARATOR) {
        return None;
    }
    let relative = format!("{VIEW_DIRECTORY}{}", name.replace('.', "/"));
    let last = relative.rsplit('/').next().unwrap_or(&relative);
    let suffixes = [format!("{relative}.blade.php"), format!("{relative}.php")];
    let mut candidates: Vec<&str> = [format!("{last}{TEMPLATE_STEM_SUFFIX}"), last.to_string()]
        .iter()
        .flat_map(|stem| resolver.project.files_with_stem(stem).iter().copied())
        .filter(|path| {
            suffixes
                .iter()
                .any(|suffix| *path == suffix || path.ends_with(&format!("/{suffix}")))
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    let shared = |path: &str| {
        path.split('/')
            .zip(resolver.path.split('/'))
            .take_while(|(left, right)| left == right)
            .count()
    };
    let Some(best) = candidates.iter().map(|path| shared(path)).max() else {
        return missing_view(name);
    };
    let nearest: Vec<&str> = candidates
        .into_iter()
        .filter(|path| shared(path) == best)
        .collect();
    match nearest.as_slice() {
        [only] => Some((Endpoint::File(only.to_string()), None)),
        [] => missing_view(name),
        several => Some((
            Endpoint::Unresolved(format!("view {name}")),
            Some(format!("ambiguous: {} candidates", several.len())),
        )),
    }
}

/// A view that matches no file, unless it is a component name that may live in a package.
fn missing_view(name: &str) -> Option<(Endpoint, Option<String>)> {
    if name.starts_with(COMPONENT_VIEW_PREFIX) {
        return None;
    }
    Some((
        Endpoint::Unresolved(format!("view {name}")),
        Some("view file not found".to_string()),
    ))
}
