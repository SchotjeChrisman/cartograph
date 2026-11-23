//! Service container bindings: `$this->app->bind(Contract::class, Implementation::class)`.

use tree_sitter::Node;

use super::arguments::{arguments, class_constant, short_name};
use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;

const BINDING_METHODS: [&str; 5] = ["bind", "singleton", "scoped", "bindIf", "singletonIf"];
const APP_FACADE: &str = "App";
const APP_PROPERTY: &str = "$this->app";

/// `App::bind(...)`
pub(super) fn facade_binding(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let scope = node
        .child_by_field_name("scope")
        .map(|scope| extractor.text(scope));
    if scope.is_some_and(|scope| short_name(&scope) == APP_FACADE) {
        push_binding(extractor, node);
    }
}

/// `$this->app->singleton(...)`
pub(super) fn member_binding(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let object = node
        .child_by_field_name("object")
        .map(|object| extractor.text(object));
    if object.as_deref() == Some(APP_PROPERTY) {
        push_binding(extractor, node);
    }
}

fn push_binding(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let is_binding = node
        .child_by_field_name("name")
        .is_some_and(|name| BINDING_METHODS.contains(&extractor.text(name).as_str()));
    let arguments = arguments(node);
    let (Some(contract), Some(implementation)) = (
        arguments
            .first()
            .and_then(|argument| class_constant(extractor, *argument)),
        arguments
            .get(1)
            .and_then(|argument| class_constant(extractor, *argument)),
    ) else {
        return;
    };
    if is_binding {
        let mut reference = extractor.reference(
            ReferenceKind::Framework(FrameworkLink::Binding),
            contract,
            node,
        );
        reference.member = Some(implementation);
        extractor.push_reference(reference);
    }
}
