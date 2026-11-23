//! Tree-sitter plumbing shared by the language modules.

use std::borrow::Cow;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::module::ExtractError;

const SIGNATURE_MAX_CHARS: usize = 200;
const DOC_MAX_CHARS: usize = 400;

pub fn parse(language: &Language, bytes: &[u8]) -> Result<Tree, ExtractError> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|error| ExtractError::Grammar(error.to_string()))?;
    parser.parse(bytes, None).ok_or(ExtractError::NoTree)
}

/// A node a query pattern captured, with the capture's name.
pub struct Capture<'query, 'tree> {
    pub name: &'query str,
    pub node: Node<'tree>,
}

/// Every capture in document order, outer nodes before the nodes inside them, so a caller can
/// keep a stack of enclosing scopes and pop it as the captures move past each scope's end.
pub fn captures_in_document_order<'query, 'tree>(
    query: &'query Query,
    root: Node<'tree>,
    source: &[u8],
) -> Vec<Capture<'query, 'tree>> {
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source);
    let mut captures = Vec::new();
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            captures.push(Capture {
                name: names[capture.index as usize],
                node: capture.node,
            });
        }
    }
    captures.sort_by_key(|capture| {
        (
            capture.node.start_byte(),
            std::cmp::Reverse(capture.node.end_byte()),
        )
    });
    captures
}

pub fn has_token(node: Node<'_>, token: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| !child.is_named() && child.kind() == token)
}

pub fn named_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn text<'source>(node: Node<'_>, source: &'source [u8]) -> Cow<'source, str> {
    String::from_utf8_lossy(&source[node.byte_range()])
}

/// One-based line the node starts on.
pub fn start_line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

pub fn end_line(node: Node<'_>) -> u32 {
    node.end_position().row as u32 + 1
}

/// Counts error and missing nodes, skipping every subtree the parser found clean.
pub fn count_syntax_errors(root: Node<'_>) -> u32 {
    if !root.has_error() {
        return 0;
    }
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            count += 1;
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor).filter(|child| child.has_error()));
    }
    count
}

/// The definition's text up to its body, on one line.
pub fn signature(node: Node<'_>, source: &[u8]) -> String {
    let end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let head = String::from_utf8_lossy(&source[node.start_byte()..end]);
    let collapsed = head.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(SIGNATURE_MAX_CHARS).collect()
}

/// The comment block directly above `node`. `is_comment` and `skip` pick the grammar's comment
/// nodes and the nodes (attributes) that may sit between comment and item.
pub fn leading_doc(
    node: Node<'_>,
    source: &[u8],
    is_comment: impl Fn(&str) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut expected_row = node.start_position().row;
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        let kind = sibling.kind();
        if skip(kind) {
            expected_row = sibling.start_position().row;
        } else if is_comment(kind) && sibling.end_position().row + 1 >= expected_row {
            lines.push(strip_comment_markers(&text(sibling, source)));
            expected_row = sibling.start_position().row;
        } else {
            break;
        }
        previous = sibling.prev_sibling();
    }
    lines.reverse();
    let doc = lines.join("\n");
    let doc: String = doc.trim().chars().take(DOC_MAX_CHARS).collect();
    (!doc.is_empty()).then_some(doc)
}

fn strip_comment_markers(comment: &str) -> String {
    comment
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches("/**")
                .trim_start_matches("/*!")
                .trim_start_matches("/*")
                .trim_end_matches("*/")
                .trim_start_matches("///")
                .trim_start_matches("//!")
                .trim_start_matches("//")
                .trim_start_matches('*')
                .trim()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes `<...>` generic argument lists, nested ones included, and whitespace.
pub fn without_generics(text: &str) -> String {
    let mut depth = 0u32;
    let mut cleaned = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth > 0 || character.is_whitespace() => {}
            _ => cleaned.push(character),
        }
    }
    cleaned.replace("::::", "::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_generics_and_turbofish() {
        assert_eq!(without_generics("S::<i32>::other"), "S::other");
        assert_eq!(without_generics("a::Foo<b::Bar<int>>::baz"), "a::Foo::baz");
        assert_eq!(without_generics("std :: vector"), "std::vector");
    }
}
