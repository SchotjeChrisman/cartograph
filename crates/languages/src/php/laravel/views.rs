//! Views rendered from PHP: `view('users.index')`, `View::make(...)` and `Route::view(...)`.

use tree_sitter::Node;

use super::arguments::{arguments, short_name, string_literal};
use crate::model::{FrameworkLink, ReferenceKind};
use crate::php::extract::Extractor;

const VIEW_HELPER: &str = "view";
const VIEW_FACADE: &str = "View";
const ROUTE_FACADE: &str = "Route";

pub(super) fn view_helper(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let is_helper = node
        .child_by_field_name("function")
        .is_some_and(|callee| extractor.text(callee) == VIEW_HELPER);
    if is_helper
        && let Some(name) = arguments(node)
            .first()
            .and_then(|argument| string_literal(extractor, *argument))
    {
        push_view(extractor, node, name);
    }
}

/// `View::make('users.index')` and `Route::view('/home', 'home')`.
pub(super) fn facade_call(extractor: &mut Extractor<'_>, node: Node<'_>) {
    let (Some(scope), Some(method)) = (
        node.child_by_field_name("scope"),
        node.child_by_field_name("name"),
    ) else {
        return;
    };
    let scope = extractor.text(scope);
    let arguments = arguments(node);
    let view = match (short_name(&scope), extractor.text(method).as_str()) {
        (VIEW_FACADE, "make") => arguments.first(),
        (ROUTE_FACADE, "view") => arguments.get(1),
        _ => None,
    };
    if let Some(name) = view.and_then(|view| string_literal(extractor, *view)) {
        push_view(extractor, node, name);
    }
}

pub(super) fn push_view(extractor: &mut Extractor<'_>, node: Node<'_>, name: String) {
    let reference = extractor.reference(ReferenceKind::Framework(FrameworkLink::View), name, node);
    extractor.push_reference(reference);
}
