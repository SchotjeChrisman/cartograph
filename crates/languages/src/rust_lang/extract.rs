use graph::NodeKind;
use tree_sitter::{Language, Node, Query};

use super::module_path::module_path;
use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{ExtractError, SourceFile};
use crate::syntax::{self, Capture};

#[derive(PartialEq, Eq)]
enum FrameKind {
    Module,
    /// An `impl` block; its prefix is the implementing type's qualified name.
    Impl,
    Trait,
    Item,
}

/// An item or block whose braces are still open at the current capture.
struct Frame {
    end_byte: usize,
    definition: Option<usize>,
    prefix: String,
    kind: FrameKind,
}

struct Extractor<'source> {
    source: &'source [u8],
    file_module: String,
    extraction: Extraction,
    frames: Vec<Frame>,
}

pub(super) fn extract(
    language: &Language,
    query: &Query,
    file: &SourceFile<'_>,
) -> Result<Extraction, ExtractError> {
    let tree = syntax::parse(language, file.bytes)?;
    let root = tree.root_node();
    let segments = module_path(file.path, file.context);
    let mut extractor = Extractor {
        source: file.bytes,
        file_module: segments.join("::"),
        extraction: Extraction {
            syntax_errors: syntax::count_syntax_errors(root),
            ..Extraction::default()
        },
        frames: Vec::new(),
    };
    extractor.define_file_module(&segments, root);
    for capture in syntax::captures_in_document_order(query, root, file.bytes) {
        extractor.visit(&capture);
    }
    extractor.extraction.attach_members_to_types();
    extractor.extraction.disambiguate_duplicates();
    Ok(extractor.extraction)
}

impl Extractor<'_> {
    /// Every file is the body of one module; defining it lets `use` and `mod` resolve to files
    /// the same way they resolve to items.
    fn define_file_module(&mut self, segments: &[String], root: Node<'_>) {
        self.extraction.definitions.push(Definition {
            kind: NodeKind::Module,
            name: segments.last().cloned().unwrap_or_default(),
            qualified_name: self.file_module.clone(),
            line: 1,
            end_line: syntax::end_line(root),
            signature: None,
            doc: None,
            parent: None,
            arity: None,
        });
    }

    fn visit(&mut self, capture: &Capture<'_, '_>) {
        let node = capture.node;
        while self
            .frames
            .last()
            .is_some_and(|frame| frame.end_byte <= node.start_byte())
        {
            self.frames.pop();
        }
        match capture.name {
            "module" => self.module(node),
            "class" => self.item(node, NodeKind::Class),
            "type" => self.item(node, NodeKind::Type),
            "trait" => self.trait_item(node),
            "impl" => self.implementation(node),
            "function" => self.function(node),
            "use" => self.use_declaration(node),
            "call" => self.call(node),
            _ => {}
        }
    }

    fn prefix(&self) -> &str {
        self.frames
            .last()
            .map_or(self.file_module.as_str(), |frame| frame.prefix.as_str())
    }

    fn module_scope(&self) -> String {
        self.frames
            .iter()
            .rev()
            .find(|frame| frame.kind == FrameKind::Module)
            .map_or(self.file_module.clone(), |frame| frame.prefix.clone())
    }

    fn receiver_type(&self) -> Option<String> {
        self.frames
            .iter()
            .rev()
            .find(|frame| matches!(frame.kind, FrameKind::Impl | FrameKind::Trait))
            .map(|frame| frame.prefix.clone())
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

    fn module(&mut self, node: Node<'_>) {
        let Some(name) = self.name_of(node) else {
            return;
        };
        if node.child_by_field_name("body").is_none() {
            self.push_reference(ReferenceKind::ModuleDeclaration, name, None, node, None);
            return;
        }
        let index = self.define(node, NodeKind::Module, &name, None);
        self.open_frame(node, index, FrameKind::Module);
    }

    fn item(&mut self, node: Node<'_>, kind: NodeKind) {
        if let Some(name) = self.name_of(node) {
            self.define(node, kind, &name, None);
        }
    }

    fn trait_item(&mut self, node: Node<'_>) {
        let Some(name) = self.name_of(node) else {
            return;
        };
        let scope = self.prefix().to_string();
        let index = self.define(node, NodeKind::Type, &name, None);
        self.open_frame(node, index, FrameKind::Trait);
        let Some(bounds) = node.child_by_field_name("bounds") else {
            return;
        };
        let mut cursor = bounds.walk();
        for bound in bounds.named_children(&mut cursor) {
            if bound.kind() == "lifetime" {
                continue;
            }
            let text = syntax::without_generics(&syntax::text(bound, self.source));
            let mut reference = self.reference(ReferenceKind::Inherits, text, bound);
            reference.from = Some(index);
            reference.scope = scope.clone();
            self.extraction.references.push(reference);
        }
    }

    fn implementation(&mut self, node: Node<'_>) {
        let Some(type_node) = node.child_by_field_name("type") else {
            return;
        };
        let type_name = type_name_of(&syntax::text(type_node, self.source));
        let scope = self.module_scope();
        let type_qualified_name = format!("{scope}::{type_name}");
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: None,
            prefix: type_qualified_name.clone(),
            kind: FrameKind::Impl,
        });
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let text = syntax::without_generics(&syntax::text(trait_node, self.source));
            let mut reference = self.reference(ReferenceKind::Implements, text, node);
            reference.receiver_type = Some(type_qualified_name);
            reference.scope = scope;
            self.extraction.references.push(reference);
        }
    }

    fn function(&mut self, node: Node<'_>) {
        let Some(name) = self.name_of(node) else {
            return;
        };
        let in_impl_or_trait = self
            .frames
            .last()
            .is_some_and(|frame| matches!(frame.kind, FrameKind::Impl | FrameKind::Trait));
        let kind = if in_impl_or_trait {
            NodeKind::Method
        } else {
            NodeKind::Function
        };
        let arity = node.child_by_field_name("parameters").map(|parameters| {
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "parameter")
                .count() as u32
        });
        let index = self.define(node, kind, &name, arity);
        self.open_frame(node, index, FrameKind::Item);
    }

    fn use_declaration(&mut self, node: Node<'_>) {
        let Some(argument) = node.child_by_field_name("argument") else {
            return;
        };
        let mut imports = Vec::new();
        collect_imports(argument, "", self.source, &mut imports);
        for import in imports {
            let kind = ReferenceKind::Use { glob: import.glob };
            self.push_reference(kind, import.path, import.alias, node, None);
        }
    }

    fn call(&mut self, node: Node<'_>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        let callee = match callee.kind() {
            "generic_function" => callee.child_by_field_name("function").unwrap_or(callee),
            _ => callee,
        };
        let (kind, text, via_self) = match callee.kind() {
            "identifier" | "scoped_identifier" => (
                ReferenceKind::Call { method: false },
                syntax::without_generics(&syntax::text(callee, self.source)),
                false,
            ),
            "field_expression" => {
                let Some(field) = callee.child_by_field_name("field") else {
                    return;
                };
                let on_self = callee
                    .child_by_field_name("value")
                    .is_some_and(|value| value.kind() == "self");
                (
                    ReferenceKind::Call { method: true },
                    syntax::text(field, self.source).to_string(),
                    on_self,
                )
            }
            _ => return,
        };
        let arguments = node
            .child_by_field_name("arguments")
            .map(|arguments| arguments.named_child_count() as u32);
        let mut reference = self.reference(kind, text, node);
        reference.from = self.enclosing_callable();
        reference.arguments = arguments;
        reference.via_self = via_self;
        self.extraction.references.push(reference);
    }

    fn name_of(&self, node: Node<'_>) -> Option<String> {
        let name = node.child_by_field_name("name")?;
        Some(syntax::text(name, self.source).to_string())
    }

    fn reference(&self, kind: ReferenceKind, text: String, node: Node<'_>) -> Reference {
        Reference {
            kind,
            text,
            alias: None,
            member: None,
            from: None,
            line: syntax::start_line(node),
            arguments: None,
            scope: self.module_scope(),
            receiver_type: self.receiver_type(),
            via_self: false,
        }
    }

    fn push_reference(
        &mut self,
        kind: ReferenceKind,
        text: String,
        alias: Option<String>,
        node: Node<'_>,
        from: Option<usize>,
    ) {
        let mut reference = self.reference(kind, text, node);
        reference.alias = alias;
        reference.from = from;
        self.extraction.references.push(reference);
    }

    fn define(&mut self, node: Node<'_>, kind: NodeKind, name: &str, arity: Option<u32>) -> usize {
        self.extraction.definitions.push(Definition {
            kind,
            name: name.to_string(),
            qualified_name: format!("{}::{name}", self.prefix()),
            line: syntax::start_line(node),
            end_line: syntax::end_line(node),
            signature: Some(syntax::signature(node, self.source)),
            doc: syntax::leading_doc(
                node,
                self.source,
                |kind| kind == "line_comment" || kind == "block_comment",
                |kind| kind == "attribute_item",
            ),
            parent: self.enclosing_definition(),
            arity,
        });
        self.extraction.definitions.len() - 1
    }

    fn open_frame(&mut self, node: Node<'_>, definition: usize, kind: FrameKind) {
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: Some(definition),
            prefix: self.extraction.definitions[definition]
                .qualified_name
                .clone(),
            kind,
        });
    }
}

/// `&mut Foo<T>` and `dyn Foo` are both about `Foo`.
fn type_name_of(text: &str) -> String {
    let cleaned = syntax::without_generics(text);
    cleaned
        .trim_start_matches('&')
        .trim_start_matches("mut")
        .trim_start_matches("dyn")
        .to_string()
}

struct Import {
    path: String,
    alias: Option<String>,
    glob: bool,
}

/// Flattens a `use` argument such as `a::{b, c as d, e::*}` into one import per name.
fn collect_imports(node: Node<'_>, prefix: &str, source: &[u8], imports: &mut Vec<Import>) {
    let text = |node: Node<'_>| syntax::text(node, source).to_string();
    let join = |tail: &str| {
        if prefix.is_empty() {
            tail.to_string()
        } else {
            format!("{prefix}::{tail}")
        }
    };
    match node.kind() {
        "identifier" | "crate" | "super" | "scoped_identifier" => {
            let path = join(&text(node));
            let alias = path.rsplit("::").next().map(str::to_string);
            imports.push(Import {
                path,
                alias,
                glob: false,
            });
        }
        "self" => {
            let alias = prefix.rsplit("::").next().map(str::to_string);
            imports.push(Import {
                path: prefix.to_string(),
                alias,
                glob: false,
            });
        }
        "use_as_clause" => {
            let (Some(path), Some(alias)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias"),
            ) else {
                return;
            };
            let alias = text(alias);
            let alias = (alias != "_").then_some(alias);
            let full = if path.kind() == "self" {
                prefix.to_string()
            } else {
                join(&text(path))
            };
            imports.push(Import {
                path: full,
                alias,
                glob: false,
            });
        }
        "scoped_use_list" => {
            let inner = match node.child_by_field_name("path") {
                Some(path) => join(&text(path)),
                None => prefix.to_string(),
            };
            if let Some(list) = node.child_by_field_name("list") {
                collect_imports(list, &inner, source, imports);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_imports(child, prefix, source, imports);
            }
        }
        "use_wildcard" => {
            let path = node
                .named_child(0)
                .map_or_else(|| prefix.to_string(), |path| join(&text(path)));
            imports.push(Import {
                path,
                alias: None,
                glob: true,
            });
        }
        _ => {}
    }
}
