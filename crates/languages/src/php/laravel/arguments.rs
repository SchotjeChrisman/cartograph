//! Reading call arguments and literals out of PHP syntax, for the framework patterns.

use tree_sitter::Node;

use crate::php::NAMESPACE_SEPARATOR;
use crate::php::extract::Extractor;
use crate::syntax::named_children;

/// The expressions passed to a call, without the `argument` wrappers.
pub(super) fn arguments<'tree>(call: Node<'tree>) -> Vec<Node<'tree>> {
    let Some(list) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    named_children(list)
        .into_iter()
        .filter_map(|argument| named_children(argument).pop())
        .collect()
}

/// The text of a string literal without interpolation.
pub(super) fn string_literal(extractor: &Extractor<'_>, node: Node<'_>) -> Option<String> {
    match node.kind() {
        "string" => Some(extractor.text(node).trim_matches('\'').to_string()),
        "encapsed_string"
            if named_children(node)
                .iter()
                .all(|part| part.kind() == "string_content") =>
        {
            Some(extractor.text(node).trim_matches('"').to_string())
        }
        _ => None,
    }
}

/// `Class` from `Class::class`.
pub(super) fn class_constant(extractor: &Extractor<'_>, node: Node<'_>) -> Option<String> {
    if node.kind() != "class_constant_access_expression" {
        return None;
    }
    let parts = named_children(node);
    let (class, constant) = (parts.first()?, parts.last()?);
    (extractor.text(*constant) == "class" && class.id() != constant.id())
        .then(|| extractor.text(*class))
}

/// The elements of an array literal; each is its value, with the key when it has one.
pub(super) fn array_elements<'tree>(node: Node<'tree>) -> Vec<(Option<Node<'tree>>, Node<'tree>)> {
    if node.kind() != "array_creation_expression" {
        return Vec::new();
    }
    named_children(node)
        .into_iter()
        .filter(|element| element.kind() == "array_element_initializer")
        .filter_map(|element| {
            let mut parts = named_children(element);
            let value = parts.pop()?;
            Some((parts.pop(), value))
        })
        .collect()
}

/// The strings of an array literal such as `['index', 'show']`.
pub(super) fn string_list(extractor: &Extractor<'_>, node: Node<'_>) -> Vec<String> {
    array_elements(node)
        .into_iter()
        .filter_map(|(_, value)| string_literal(extractor, value))
        .collect()
}

/// The last segment of a possibly qualified name: `Facades\Route` gives `Route`.
pub(super) fn short_name(name: &str) -> &str {
    name.rsplit(NAMESPACE_SEPARATOR).next().unwrap_or(name)
}
