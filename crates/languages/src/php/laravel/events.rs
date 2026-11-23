//! Events and listeners as hooks: code that fires an event, and the listeners registered for it.

use tree_sitter::Node;

use super::arguments::{arguments, array_elements, class_constant, short_name};
use crate::model::{FrameworkLink, Reference, ReferenceKind};
use crate::php::extract::Extractor;
use crate::syntax::named_children;

const EVENT_HELPER: &str = "event";
const DISPATCHING_FACADES: [&str; 2] = ["Event", "Bus"];
const LISTEN_PROPERTY: &str = "$listen";
const LISTENER_DIRECTORY: &str = "/Listeners/";
const HANDLER_METHOD: &str = "handle";

/// `event(new UserCreated($user))`
pub(super) fn event_helper(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let is_helper = node
        .child_by_field_name("function")
        .is_some_and(|callee| extractor.text(callee) == EVENT_HELPER);
    if is_helper
        && let Some(event) = arguments(node)
            .first()
            .and_then(|argument| constructed_class(extractor, *argument))
    {
        push_fired(extractor, node, event);
    }
}

/// `UserCreated::dispatch(...)`, `Event::dispatch(new UserCreated)` and `Event::listen(...)`.
pub(super) fn facade_call(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let (Some(scope), Some(name)) = (
        node.child_by_field_name("scope"),
        node.child_by_field_name("name"),
    ) else {
        return;
    };
    let scope = extractor.text(scope);
    let facade = short_name(&scope);
    let arguments = arguments(node);
    match (
        extractor.text(name).as_str(),
        DISPATCHING_FACADES.contains(&facade),
    ) {
        ("dispatch", true) => {
            if let Some(event) = arguments
                .first()
                .and_then(|argument| constructed_class(extractor, *argument))
            {
                push_fired(extractor, node, event);
            }
        }
        ("dispatch", false) => push_fired(extractor, node, scope),
        ("listen", true) => register_listener(extractor, node, &arguments),
        _ => {}
    }
}

/// `Event::listen(UserCreated::class, SendMail::class)` or `[SendMail::class, 'handle']`.
fn register_listener(extractor: &mut Extractor<'_>, node: Node<'_>, arguments: &[Node<'_>]) {
    let (Some(event), Some(listener)) = (arguments.first(), arguments.get(1)) else {
        return;
    };
    let listener_class = class_constant(extractor, *listener).or_else(|| {
        array_elements(*listener)
            .first()
            .and_then(|(_, class)| class_constant(extractor, *class))
    });
    if let (Some(event), Some(listener)) = (class_constant(extractor, *event), listener_class) {
        push_listener(extractor, node, listener, event);
    }
}

/// `protected $listen = [UserCreated::class => [SendMail::class]];`
pub(super) fn listen_property(extractor: &mut Extractor<'_>, node: Node<'_>) {
    for element in named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "property_element")
    {
        let is_listen = element
            .child_by_field_name("name")
            .is_some_and(|name| extractor.text(name) == LISTEN_PROPERTY);
        let Some(map) = element
            .child_by_field_name("default_value")
            .filter(|_| is_listen)
        else {
            continue;
        };
        for (event, listeners) in array_elements(map) {
            let Some(event) = event.and_then(|event| class_constant(extractor, event)) else {
                continue;
            };
            for (_, listener) in array_elements(listeners) {
                if let Some(class) = class_constant(extractor, listener) {
                    push_listener(extractor, listener, class, event.clone());
                }
            }
        }
    }
}

/// A `handle(UserCreated $event)` method in a listeners directory is registered by convention.
pub(super) fn handler_method(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let is_handler = node
        .child_by_field_name("name")
        .is_some_and(|name| extractor.text(name) == HANDLER_METHOD);
    if !is_handler || !extractor.path.contains(LISTENER_DIRECTORY) {
        return;
    }
    let first_parameter = node
        .child_by_field_name("parameters")
        .and_then(|parameters| named_children(parameters).into_iter().next());
    let event = first_parameter
        .and_then(|parameter| parameter.child_by_field_name("type"))
        .filter(|type_node| type_node.kind() == "named_type")
        .and_then(|type_node| type_node.named_child(0));
    if let (Some(event), Some(method)) = (event, extractor.enclosing_definition()) {
        let mut reference = extractor.reference(
            ReferenceKind::Framework(FrameworkLink::HookHandled),
            extractor.text(event),
            node,
        );
        reference.from = Some(method);
        extractor.push_reference(reference);
    }
}

fn constructed_class(extractor: &Extractor<'_>, node: Node<'_>) -> Option<String> {
    if node.kind() != "object_creation_expression" {
        return None;
    }
    named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "name" | "qualified_name"))
        .map(|class| extractor.text(class))
}

fn push_fired(extractor: &mut Extractor<'_>, node: Node<'_>, event: String) {
    let reference = extractor.reference(
        ReferenceKind::Framework(FrameworkLink::HookFired),
        event,
        node,
    );
    extractor.push_reference(reference);
}

fn push_listener(extractor: &mut Extractor<'_>, node: Node<'_>, listener: String, event: String) {
    let mut reference: Reference = extractor.reference(
        ReferenceKind::Framework(FrameworkLink::Listener),
        listener,
        node,
    );
    reference.member = Some(event);
    extractor.push_reference(reference);
}
