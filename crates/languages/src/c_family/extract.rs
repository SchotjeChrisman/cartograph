use graph::NodeKind;
use tree_sitter::{Language, Node, Query};

use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{ExtractError, SourceFile};
use crate::syntax::{self, Capture};

/// A definition or namespace whose braces are still open at the current capture.
struct Frame {
    end_byte: usize,
    definition: Option<usize>,
    prefix: String,
}

struct Extractor<'source> {
    source: &'source [u8],
    extraction: Extraction,
    frames: Vec<Frame>,
}

pub(super) fn extract(
    language: &Language,
    query: &Query,
    has_namespaces: bool,
    file: &SourceFile<'_>,
) -> Result<Extraction, ExtractError> {
    let tree = syntax::parse(language, file.bytes)?;
    let root = tree.root_node();
    let mut extractor = Extractor {
        source: file.bytes,
        extraction: Extraction {
            syntax_errors: syntax::count_syntax_errors(root),
            ..Extraction::default()
        },
        frames: Vec::new(),
    };
    for capture in syntax::captures_in_document_order(query, root, file.bytes) {
        extractor.visit(&capture, has_namespaces);
    }
    extractor.extraction.attach_members_to_types();
    extractor.extraction.disambiguate_duplicates();
    Ok(extractor.extraction)
}

impl Extractor<'_> {
    fn visit(&mut self, capture: &Capture<'_, '_>, has_namespaces: bool) {
        let node = capture.node;
        while self
            .frames
            .last()
            .is_some_and(|frame| frame.end_byte <= node.start_byte())
        {
            self.frames.pop();
        }
        match capture.name {
            "include" => self.include(node),
            "namespace" if has_namespaces => self.namespace(node),
            "class" => self.class(node),
            "enum" | "alias" => self.named_type(node, "name"),
            "typedef" => self.typedef(node),
            "function" => self.function(node),
            "call" => self.call(node),
            _ => {}
        }
    }

    fn prefix(&self) -> &str {
        self.frames.last().map_or("", |frame| frame.prefix.as_str())
    }

    fn enclosing_definition(&self) -> Option<usize> {
        self.frames.iter().rev().find_map(|frame| frame.definition)
    }

    fn enclosing_callable(&self) -> Option<usize> {
        self.frames
            .iter()
            .rev()
            .filter_map(|frame| frame.definition)
            .find(|&index| {
                matches!(
                    self.extraction.definitions[index].kind,
                    NodeKind::Function | NodeKind::Method
                )
            })
    }

    fn scope_of_reference(&self) -> String {
        match self.enclosing_callable() {
            Some(index) => {
                qualifier(&self.extraction.definitions[index].qualified_name).to_string()
            }
            None => self.prefix().to_string(),
        }
    }

    fn include(&mut self, node: Node<'_>) {
        let Some(path) = node.child_by_field_name("path") else {
            return;
        };
        let text = syntax::text(path, self.source);
        let system = path.kind() == "system_lib_string";
        let trimmed = text.trim_matches(|c| matches!(c, '"' | '<' | '>'));
        self.extraction.references.push(Reference {
            kind: ReferenceKind::Include { system },
            text: trimmed.to_string(),
            alias: None,
            member: None,
            from: None,
            line: syntax::start_line(node),
            arguments: None,
            scope: String::new(),
            receiver_type: None,
            via_self: false,
        });
    }

    fn namespace(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            self.push_frame(node, None, self.prefix().to_string());
            return;
        };
        let name = clean_name(&syntax::text(name, self.source));
        let index = self.define(node, NodeKind::Module, &name, None);
        let prefix = self.extraction.definitions[index].qualified_name.clone();
        self.push_frame(node, Some(index), prefix);
    }

    fn class(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            self.push_frame(node, None, self.prefix().to_string());
            return;
        };
        let name = clean_name(&syntax::text(name, self.source));
        let index = self.define(node, NodeKind::Class, &name, None);
        let prefix = self.extraction.definitions[index].qualified_name.clone();
        let scope = self.prefix().to_string();
        self.push_frame(node, Some(index), prefix);
        self.base_classes(node, index, &scope);
    }

    fn base_classes(&mut self, class: Node<'_>, index: usize, scope: &str) {
        let mut cursor = class.walk();
        let Some(clause) = class
            .children(&mut cursor)
            .find(|child| child.kind() == "base_class_clause")
        else {
            return;
        };
        let mut clause_cursor = clause.walk();
        for base in clause.named_children(&mut clause_cursor) {
            if base.kind() == "access_specifier" {
                continue;
            }
            let text = syntax::without_generics(&syntax::text(base, self.source));
            self.extraction.references.push(Reference {
                kind: ReferenceKind::Inherits,
                text,
                alias: None,
                member: None,
                from: Some(index),
                line: syntax::start_line(base),
                arguments: None,
                scope: scope.to_string(),
                receiver_type: None,
                via_self: false,
            });
        }
    }

    fn named_type(&mut self, node: Node<'_>, name_field: &str) {
        if let Some(name) = node.child_by_field_name(name_field) {
            let name = clean_name(&syntax::text(name, self.source));
            self.define(node, NodeKind::Type, &name, None);
        }
    }

    fn typedef(&mut self, node: Node<'_>) {
        let Some(declarator) = node.child_by_field_name("declarator") else {
            return;
        };
        if declarator.kind() == "type_identifier" {
            let name = syntax::text(declarator, self.source).to_string();
            self.define(node, NodeKind::Type, &name, None);
        }
    }

    fn function(&mut self, node: Node<'_>) {
        let Some(declarator) = node
            .child_by_field_name("declarator")
            .and_then(function_declarator)
        else {
            return;
        };
        let Some(name_node) = declarator.child_by_field_name("declarator") else {
            return;
        };
        let written = clean_name(&syntax::text(name_node, self.source));
        let in_class = self
            .enclosing_definition()
            .is_some_and(|index| self.extraction.definitions[index].kind == NodeKind::Class);
        let kind = if in_class || written.contains("::") {
            NodeKind::Method
        } else {
            NodeKind::Function
        };
        let arity = declarator
            .child_by_field_name("parameters")
            .map(|parameters| parameter_count(parameters, self.source));
        let index = self.define(node, kind, &written, arity);
        let prefix = self.extraction.definitions[index].qualified_name.clone();
        self.push_frame(node, Some(index), prefix);
    }

    fn call(&mut self, node: Node<'_>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        let (text, method) = match callee.kind() {
            "identifier" | "qualified_identifier" => (
                syntax::without_generics(&syntax::text(callee, self.source)),
                false,
            ),
            "template_function" => match callee.child_by_field_name("name") {
                Some(name) => (syntax::text(name, self.source).to_string(), false),
                None => return,
            },
            "field_expression" => match callee.child_by_field_name("field") {
                Some(field) => (
                    syntax::without_generics(&syntax::text(field, self.source)),
                    true,
                ),
                None => return,
            },
            _ => return,
        };
        let arguments = node
            .child_by_field_name("arguments")
            .map(|arguments| arguments.named_child_count() as u32);
        self.extraction.references.push(Reference {
            kind: ReferenceKind::Call { method },
            text: text.trim_start_matches("::").to_string(),
            alias: None,
            member: None,
            from: self.enclosing_callable(),
            line: syntax::start_line(node),
            arguments,
            scope: self.scope_of_reference(),
            receiver_type: None,
            via_self: false,
        });
    }

    /// Adds a definition under the current scope and returns its index.
    fn define(
        &mut self,
        node: Node<'_>,
        kind: NodeKind,
        written: &str,
        arity: Option<u32>,
    ) -> usize {
        let written = written.trim_start_matches("::");
        let name = written.rsplit("::").next().unwrap_or(written).to_string();
        let qualified_name = match self.prefix() {
            "" => written.to_string(),
            prefix => format!("{prefix}::{written}"),
        };
        let doc_anchor = match node.parent() {
            Some(parent) if parent.kind() == "template_declaration" => parent,
            _ => node,
        };
        self.extraction.definitions.push(Definition {
            kind,
            name,
            qualified_name,
            line: syntax::start_line(node),
            end_line: syntax::end_line(node),
            signature: Some(syntax::signature(node, self.source)),
            doc: syntax::leading_doc(doc_anchor, self.source, is_comment, |_| false),
            parent: self.enclosing_definition(),
            arity,
        });
        self.extraction.definitions.len() - 1
    }

    fn push_frame(&mut self, node: Node<'_>, definition: Option<usize>, prefix: String) {
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition,
            prefix,
        });
    }
}

fn is_comment(kind: &str) -> bool {
    kind == "comment"
}

/// The enclosing scope of a qualified name: `a::b::f` gives `a::b`.
pub(super) fn qualifier(qualified_name: &str) -> &str {
    qualified_name
        .rsplit_once("::")
        .map_or("", |(scope, _)| scope)
}

/// Strips generics, except in operator names where `<` is part of the name.
fn clean_name(text: &str) -> String {
    if text.contains("operator") {
        text.split_whitespace().collect::<Vec<_>>().join("")
    } else {
        syntax::without_generics(text)
    }
}

/// Finds the `function_declarator` under pointer, reference and parenthesised wrappers.
fn function_declarator(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        if node.kind() == "function_declarator" {
            return Some(node);
        }
        let last_named = node
            .named_child_count()
            .checked_sub(1)
            .and_then(|index| node.named_child(index as u32));
        node = node.child_by_field_name("declarator").or(last_named)?;
    }
}

fn parameter_count(parameters: Node<'_>, source: &[u8]) -> u32 {
    let mut cursor = parameters.walk();
    let declared: Vec<Node<'_>> = parameters
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect();
    let only_void = declared.len() == 1 && syntax::text(declared[0], source).trim() == "void";
    if only_void { 0 } else { declared.len() as u32 }
}
