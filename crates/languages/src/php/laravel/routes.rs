//! `Route::get(...)`, groups with prefixes and controllers, and resource routes.

use graph::NodeKind;
use tree_sitter::Node;

use super::arguments::{
    arguments, array_elements, class_constant, short_name, string_list, string_literal,
};
use super::inflect;
use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;

const FACADE: &str = "Route";
const HTTP_VERBS: [&str; 7] = ["get", "post", "put", "patch", "delete", "options", "any"];
const INVOKABLE_ACTION: &str = "__invoke";

/// Resource actions in Laravel's order: HTTP methods, path below the resource, action name.
const RESOURCE_ACTIONS: [(&str, &str, &str); 7] = [
    ("GET", "", "index"),
    ("GET", "/create", "create"),
    ("POST", "", "store"),
    ("GET", "/{param}", "show"),
    ("GET", "/{param}/edit", "edit"),
    ("PUT|PATCH", "/{param}", "update"),
    ("DELETE", "/{param}", "destroy"),
];
const ACTIONS_WITHOUT_FORMS: [&str; 2] = ["create", "edit"];

/// A `Route::...->group(...)` whose closure is still open.
pub(super) struct RouteGroup {
    pub(super) end_byte: usize,
    prefix: String,
    controller: Option<String>,
}

/// One call in a method chain such as `Route::prefix('a')->get(...)`.
struct Link<'tree> {
    name: String,
    arguments: Vec<Node<'tree>>,
}

struct Chain<'tree> {
    facade: String,
    links: Vec<Link<'tree>>,
}

fn chain_of<'tree>(extractor: &Extractor<'_>, node: Node<'tree>) -> Option<Chain<'tree>> {
    let name = extractor.text(node.child_by_field_name("name")?);
    let link = Link {
        name,
        arguments: arguments(node),
    };
    match node.kind() {
        "scoped_call_expression" => {
            let scope = extractor.text(node.child_by_field_name("scope")?);
            Some(Chain {
                facade: short_name(&scope).to_string(),
                links: vec![link],
            })
        }
        "member_call_expression" => {
            let mut chain = chain_of(extractor, node.child_by_field_name("object")?)?;
            chain.links.push(link);
            Some(chain)
        }
        _ => None,
    }
}

pub(super) fn visit_call(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let Some(chain) = chain_of(extractor, node) else {
        return;
    };
    if chain.facade != FACADE {
        return;
    }
    let Some((last, before)) = chain.links.split_last() else {
        return;
    };
    let uses_resource = chain.links.iter().any(|link| is_resource(&link.name));
    if uses_resource {
        if is_outermost(node) {
            register_resource(extractor, node, &chain);
        }
        return;
    }
    match last.name.as_str() {
        "group" => open_group(extractor, node, last, before),
        "match" => register_matching(extractor, node, last, before),
        verb if HTTP_VERBS.contains(&verb) => {
            let method = verb.to_ascii_uppercase();
            register_route(
                extractor,
                node,
                &method,
                last.arguments.first(),
                last.arguments.get(1),
                before,
            );
        }
        _ => {}
    }
}

fn is_resource(name: &str) -> bool {
    matches!(name, "resource" | "apiResource")
}

/// Whether no further call is chained onto `node`.
fn is_outermost(node: Node<'_>) -> bool {
    !node.parent().is_some_and(|parent| {
        parent.kind() == "member_call_expression"
            && parent
                .child_by_field_name("object")
                .is_some_and(|object| object.id() == node.id())
    })
}

fn register_matching(
    extractor: &mut Extractor<'_>,
    node: Node<'_>,
    last: &Link<'_>,
    before: &[Link<'_>],
) {
    let methods = last
        .arguments
        .first()
        .map(|methods| string_list(extractor, *methods))
        .unwrap_or_default();
    let methods: Vec<String> = methods
        .iter()
        .map(|method| method.to_ascii_uppercase())
        .collect();
    register_route(
        extractor,
        node,
        &methods.join("|"),
        last.arguments.get(1),
        last.arguments.get(2),
        before,
    );
}

fn open_group(extractor: &mut Extractor<'_>, node: Node<'_>, last: &Link<'_>, before: &[Link<'_>]) {
    let (own_prefix, own_controller) = match last.arguments.first() {
        Some(options) if options.kind() == "array_creation_expression" => {
            group_options(extractor, *options)
        }
        _ => (None, None),
    };
    let (chain_prefix, chain_controller) = modifiers(extractor, before);
    let outer = extractor
        .laravel
        .as_ref()
        .and_then(|plugin| plugin.route_groups.last());
    let prefix = join_uri(
        outer.map_or("", |group| group.prefix.as_str()),
        &[chain_prefix, own_prefix]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("/"),
    );
    let controller = own_controller
        .or(chain_controller)
        .or_else(|| outer.and_then(|group| group.controller.clone()));
    if let Some(plugin) = extractor.laravel.as_mut() {
        plugin.route_groups.push(RouteGroup {
            end_byte: node.end_byte(),
            prefix,
            controller,
        });
    }
}

/// `prefix` and `controller` from `Route::group(['prefix' => 'api', ...], ...)`.
fn group_options(extractor: &Extractor<'_>, options: Node<'_>) -> (Option<String>, Option<String>) {
    let mut prefix = None;
    let mut controller = None;
    for (key, value) in array_elements(options) {
        match key
            .and_then(|key| string_literal(extractor, key))
            .as_deref()
        {
            Some("prefix") => prefix = string_literal(extractor, value),
            Some("controller") => controller = class_constant(extractor, value),
            _ => {}
        }
    }
    (prefix, controller)
}

/// The URI prefix and default controller set by `->prefix(...)` and `->controller(...)` links.
fn modifiers(extractor: &Extractor<'_>, links: &[Link<'_>]) -> (Option<String>, Option<String>) {
    let mut prefixes = Vec::new();
    let mut controller = None;
    for link in links {
        let first = link.arguments.first();
        match link.name.as_str() {
            "prefix" => {
                prefixes.extend(first.and_then(|argument| string_literal(extractor, *argument)))
            }
            "controller" => {
                controller = first.and_then(|argument| class_constant(extractor, *argument))
            }
            _ => {}
        }
    }
    (
        (!prefixes.is_empty()).then(|| prefixes.join("/")),
        controller,
    )
}

fn register_route(
    extractor: &mut Extractor<'_>,
    node: Node<'_>,
    methods: &str,
    uri: Option<&Node<'_>>,
    handler: Option<&Node<'_>>,
    before: &[Link<'_>],
) {
    let Some(uri) = uri.and_then(|uri| string_literal(extractor, *uri)) else {
        return;
    };
    let (chain_prefix, chain_controller) = modifiers(extractor, before);
    let group = extractor
        .laravel
        .as_ref()
        .and_then(|plugin| plugin.route_groups.last());
    let controller = chain_controller.or_else(|| group.and_then(|group| group.controller.clone()));
    let prefix = join_uri(
        group.map_or("", |group| group.prefix.as_str()),
        chain_prefix.as_deref().unwrap_or(""),
    );
    let full_uri = join_uri(&prefix, &uri);
    let route = define_route(extractor, node, methods, &full_uri);
    if let Some((class, action)) =
        handler.and_then(|handler| handler_of(extractor, *handler, controller))
    {
        link_handler(extractor, node, route, class, action, false);
    }
}

fn register_resource(extractor: &mut Extractor<'_>, node: Node<'_>, chain: &Chain<'_>) {
    let Some(position) = chain.links.iter().position(|link| is_resource(&link.name)) else {
        return;
    };
    let resource = &chain.links[position];
    let only_forms = resource.name == "apiResource";
    let (Some(name), Some(class)) = (
        resource
            .arguments
            .first()
            .and_then(|name| string_literal(extractor, *name)),
        resource
            .arguments
            .get(1)
            .and_then(|class| class_constant(extractor, *class)),
    ) else {
        return;
    };
    let (chain_prefix, _) = modifiers(extractor, &chain.links[..position]);
    let group_prefix = extractor
        .laravel
        .as_ref()
        .and_then(|plugin| plugin.route_groups.last())
        .map_or("", |group| group.prefix.as_str());
    let base = join_uri(
        &join_uri(group_prefix, chain_prefix.as_deref().unwrap_or("")),
        &resource_path(&name),
    );
    let (only, except) = action_filters(extractor, &chain.links[position + 1..]);
    let param = inflect::singular(name.rsplit('.').next().unwrap_or(&name));
    for (methods, path, action) in RESOURCE_ACTIONS {
        let wanted = only
            .as_ref()
            .is_none_or(|only| only.iter().any(|name| name == action))
            && !except.iter().any(|name| name == action)
            && !(only_forms && ACTIONS_WITHOUT_FORMS.contains(&action));
        if wanted {
            let uri = format!("{base}{}", path.replace("{param}", &format!("{{{param}}}")));
            let route = define_route(extractor, node, methods, &uri);
            link_handler(
                extractor,
                node,
                route,
                class.clone(),
                action.to_string(),
                true,
            );
        }
    }
}

/// `photos.comments` is `photos/{photo}/comments`.
fn resource_path(name: &str) -> String {
    let parts: Vec<&str> = name.split('.').collect();
    let (last, parents) = parts.split_last().unwrap_or((&name, &[]));
    let mut path: Vec<String> = parents
        .iter()
        .map(|parent| format!("{parent}/{{{}}}", inflect::singular(parent)))
        .collect();
    path.push(last.to_string());
    path.join("/")
}

/// `->only([...])` and `->except([...])` after a resource call.
fn action_filters(
    extractor: &Extractor<'_>,
    links: &[Link<'_>],
) -> (Option<Vec<String>>, Vec<String>) {
    let mut only = None;
    let mut except = Vec::new();
    for link in links {
        let names = link
            .arguments
            .first()
            .map(|list| string_list(extractor, *list))
            .unwrap_or_default();
        match link.name.as_str() {
            "only" => only = Some(names),
            "except" => except = names,
            _ => {}
        }
    }
    (only, except)
}

/// The controller class and action a route handler argument names.
fn handler_of(
    extractor: &Extractor<'_>,
    handler: Node<'_>,
    controller: Option<String>,
) -> Option<(String, String)> {
    if let Some(class) = class_constant(extractor, handler) {
        return Some((class, INVOKABLE_ACTION.to_string()));
    }
    if let Some(text) = string_literal(extractor, handler) {
        return match text.split_once('@') {
            Some((class, action)) => Some((class.to_string(), action.to_string())),
            None => Some((controller?, text)),
        };
    }
    let elements = array_elements(handler);
    let [(_, class), (_, action)] = elements.as_slice() else {
        return None;
    };
    Some((
        class_constant(extractor, *class)?,
        string_literal(extractor, *action)?,
    ))
}

fn define_route(extractor: &mut Extractor<'_>, node: Node<'_>, methods: &str, uri: &str) -> usize {
    let name = format!("{methods} {uri}");
    extractor.define(node, NodeKind::Route, &name, name.clone(), None)
}

fn link_handler(
    extractor: &mut Extractor<'_>,
    node: Node<'_>,
    route: usize,
    class: String,
    action: String,
    optional: bool,
) {
    let mut reference = extractor.reference(
        ReferenceKind::Framework(FrameworkLink::RouteHandler { optional }),
        class,
        node,
    );
    reference.from = Some(route);
    reference.member = Some(action);
    extractor.push_reference(reference);
}

/// Joins URI parts with single slashes under a leading one.
fn join_uri(base: &str, tail: &str) -> String {
    let parts: Vec<&str> = [base, tail]
        .iter()
        .flat_map(|part| part.split('/'))
        .filter(|part| !part.is_empty())
        .collect();
    format!("/{}", parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_join_under_one_leading_slash() {
        assert_eq!(join_uri("", "users/{id}"), "/users/{id}");
        assert_eq!(join_uri("/api/", "/users"), "/api/users");
        assert_eq!(join_uri("", ""), "/");
    }

    #[test]
    fn nested_resources_carry_the_parent_parameter() {
        assert_eq!(resource_path("photos.comments"), "photos/{photo}/comments");
        assert_eq!(resource_path("photos"), "photos");
    }
}
