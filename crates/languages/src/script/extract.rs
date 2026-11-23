use graph::NodeKind;
use tree_sitter::{Language, Node, Query};

use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{ExtractError, SourceFile};
use crate::syntax::{self, Capture, has_token, named_child_of_kind, named_children};

const DEFAULT_KEYWORD: &str = "default";

/// A definition whose braces are still open at the current capture.
struct Frame {
    end_byte: usize,
    definition: usize,
}

/// How a call names its callee.
#[derive(PartialEq, Eq)]
enum CallShape {
    /// `f()`, `Type.member()` and `new Type()`: a name that scopes and imports can settle.
    Path,
    /// `this.member()`.
    OnThis,
    /// `value.member()`: the receiver's type is unknown.
    Method,
}

struct Extractor<'source> {
    source: &'source [u8],
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
    let mut extractor = Extractor {
        source: file.bytes,
        extraction: Extraction {
            syntax_errors: syntax::count_syntax_errors(root),
            ..Extraction::default()
        },
        frames: Vec::new(),
    };
    for capture in syntax::captures_in_document_order(query, root, file.bytes) {
        extractor.visit(&capture);
    }
    extractor.extraction.disambiguate_duplicates();
    Ok(extractor.extraction)
}

impl Extractor<'_> {
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
            "import" => self.import(node),
            "export" => self.export(node),
            "call" => self.call(node),
            "new" => self.construction(node),
            "jsx" => self.component_use(node),
            "class" => self.class(node),
            "interface" => self.named_type(node, NodeKind::Type),
            "type_alias" => self.named_type(node, NodeKind::Type),
            "enum" => self.named_type(node, NodeKind::Class),
            "namespace" => self.namespace(node),
            "function" => self.function(node, NodeKind::Function),
            "method" => self.function(node, NodeKind::Method),
            "variable_function" => self.assigned_function(node, NodeKind::Function),
            "field_function" => self.assigned_function(node, NodeKind::Method),
            _ => {}
        }
    }

    fn prefix(&self) -> &str {
        self.frames.last().map_or("", |frame| {
            self.extraction.definitions[frame.definition]
                .qualified_name
                .as_str()
        })
    }

    fn enclosing_definition(&self) -> Option<usize> {
        self.frames.last().map(|frame| frame.definition)
    }

    fn enclosing_class(&self) -> Option<&str> {
        self.frames
            .iter()
            .rev()
            .map(|frame| &self.extraction.definitions[frame.definition])
            .find(|definition| definition.kind == NodeKind::Class)
            .map(|definition| definition.qualified_name.as_str())
    }

    fn reference(&self, kind: ReferenceKind, text: String, node: Node<'_>) -> Reference {
        Reference {
            kind,
            text,
            alias: None,
            member: None,
            from: self.enclosing_definition(),
            line: syntax::start_line(node),
            arguments: None,
            scope: self.prefix().to_string(),
            receiver_type: self.enclosing_class().map(str::to_string),
            via_self: false,
        }
    }

    fn import(&mut self, node: Node<'_>) {
        let Some(specifier) = self.specifier(node) else {
            return;
        };
        let kind = ReferenceKind::Import { reexport: false };
        let Some(clause) = named_child_of_kind(node, "import_clause") else {
            let reference = self.reference(kind, specifier, node);
            self.extraction.references.push(reference);
            return;
        };
        let mut cursor = clause.walk();
        for part in clause.named_children(&mut cursor) {
            match part.kind() {
                "identifier" => self.bind(kind, &specifier, Some("default"), &part, node),
                "namespace_import" => {
                    if let Some(name) = named_child_of_kind(part, "identifier") {
                        self.bind(kind, &specifier, None, &name, node);
                    }
                }
                "named_imports" => self.import_specifiers(kind, &specifier, part, node),
                _ => {}
            }
        }
    }

    fn import_specifiers(
        &mut self,
        kind: ReferenceKind,
        specifier: &str,
        list: Node<'_>,
        statement: Node<'_>,
    ) {
        let mut cursor = list.walk();
        for item in list.named_children(&mut cursor) {
            let Some(name) = item.child_by_field_name("name") else {
                continue;
            };
            let exported = self.text(name);
            let local = item
                .child_by_field_name("alias")
                .map_or_else(|| exported.clone(), |alias| self.text(alias));
            let mut reference = self.reference(kind, specifier.to_string(), statement);
            reference.alias = Some(local);
            reference.member = Some(exported);
            self.extraction.references.push(reference);
        }
    }

    fn bind(
        &mut self,
        kind: ReferenceKind,
        specifier: &str,
        member: Option<&str>,
        local: &Node<'_>,
        statement: Node<'_>,
    ) {
        let mut reference = self.reference(kind, specifier.to_string(), statement);
        reference.alias = Some(self.text(*local));
        reference.member = member.map(str::to_string);
        self.extraction.references.push(reference);
    }

    fn export(&mut self, node: Node<'_>) {
        if let Some(specifier) = self.specifier(node) {
            self.reexport(node, &specifier);
        } else if has_token(node, DEFAULT_KEYWORD) {
            self.default_export(node);
        }
    }

    fn reexport(&mut self, node: Node<'_>, specifier: &str) {
        let kind = ReferenceKind::Import { reexport: true };
        let Some(clause) = named_child_of_kind(node, "export_clause") else {
            let reference = self.reference(kind, specifier.to_string(), node);
            self.extraction.references.push(reference);
            return;
        };
        self.import_specifiers(kind, specifier, clause, node);
    }

    fn default_export(&mut self, node: Node<'_>) {
        let exported = node
            .child_by_field_name("value")
            .filter(|value| value.kind() == "identifier")
            .or_else(|| {
                node.child_by_field_name("declaration")
                    .and_then(|declaration| declaration.child_by_field_name("name"))
            });
        if let Some(name) = exported {
            let reference = self.reference(ReferenceKind::DefaultExport, self.text(name), node);
            self.extraction.references.push(reference);
        }
    }

    fn call(&mut self, node: Node<'_>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        let arguments = node.child_by_field_name("arguments");
        let argument_count = arguments.map(|arguments| arguments.named_child_count() as u32);
        match callee.kind() {
            "identifier" if self.text(callee) == "require" => self.require(node, arguments),
            "import" => self.dynamic_import(node, arguments),
            "identifier" => {
                self.push_call(node, self.text(callee), CallShape::Path, argument_count)
            }
            "member_expression" => self.member_call(node, callee, argument_count),
            _ => {}
        }
    }

    fn member_call(&mut self, node: Node<'_>, callee: Node<'_>, arguments: Option<u32>) {
        let (Some(object), Some(property)) = (
            callee.child_by_field_name("object"),
            callee.child_by_field_name("property"),
        ) else {
            return;
        };
        let property = self.text(property);
        match object.kind() {
            "this" => self.push_call(node, property, CallShape::OnThis, arguments),
            "identifier" => self.push_call(
                node,
                format!("{}::{property}", self.text(object)),
                CallShape::Path,
                arguments,
            ),
            _ => self.push_call(node, property, CallShape::Method, arguments),
        }
    }

    fn push_call(
        &mut self,
        node: Node<'_>,
        text: String,
        shape: CallShape,
        arguments: Option<u32>,
    ) {
        let mut reference = self.reference(
            ReferenceKind::Call {
                method: shape != CallShape::Path,
            },
            text,
            node,
        );
        reference.via_self = shape == CallShape::OnThis;
        reference.arguments = arguments;
        self.extraction.references.push(reference);
    }

    fn construction(&mut self, node: Node<'_>) {
        let Some(constructor) = node.child_by_field_name("constructor") else {
            return;
        };
        let text = match constructor.kind() {
            "identifier" => self.text(constructor),
            "member_expression" => dotted_to_path(&self.text(constructor)),
            _ => return,
        };
        let arguments = node
            .child_by_field_name("arguments")
            .map(|arguments| arguments.named_child_count() as u32);
        self.push_call(node, text, CallShape::Path, arguments);
    }

    /// A capitalised JSX tag renders a component defined elsewhere, which is as good as a call.
    fn component_use(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let text = self.text(name);
        if name.kind() == "identifier" && text.starts_with(char::is_uppercase) {
            self.push_call(node, text, CallShape::Path, None);
        }
    }

    fn require(&mut self, node: Node<'_>, arguments: Option<Node<'_>>) {
        let Some(specifier) = arguments
            .and_then(|arguments| arguments.named_child(0))
            .and_then(|first| self.string_value(first))
        else {
            return;
        };
        let kind = ReferenceKind::Import { reexport: false };
        let Some(pattern) = node
            .parent()
            .filter(|parent| parent.kind() == "variable_declarator")
            .and_then(|declarator| declarator.child_by_field_name("name"))
        else {
            let reference = self.reference(kind, specifier, node);
            self.extraction.references.push(reference);
            return;
        };
        match pattern.kind() {
            "identifier" => self.bind(kind, &specifier, None, &pattern, node),
            "object_pattern" => self.destructured_require(kind, &specifier, pattern, node),
            _ => {}
        }
    }

    fn destructured_require(
        &mut self,
        kind: ReferenceKind,
        specifier: &str,
        pattern: Node<'_>,
        node: Node<'_>,
    ) {
        let mut cursor = pattern.walk();
        for property in pattern.named_children(&mut cursor) {
            let (member, local) = match property.kind() {
                "shorthand_property_identifier_pattern" => {
                    (self.text(property), self.text(property))
                }
                "pair_pattern" => match (
                    property.child_by_field_name("key"),
                    property.child_by_field_name("value"),
                ) {
                    (Some(key), Some(value)) if value.kind() == "identifier" => {
                        (self.text(key), self.text(value))
                    }
                    _ => continue,
                },
                _ => continue,
            };
            let mut reference = self.reference(kind, specifier.to_string(), node);
            reference.alias = Some(local);
            reference.member = Some(member);
            self.extraction.references.push(reference);
        }
    }

    fn dynamic_import(&mut self, node: Node<'_>, arguments: Option<Node<'_>>) {
        let specifier = arguments
            .and_then(|arguments| arguments.named_child(0))
            .and_then(|first| self.string_value(first));
        if let Some(specifier) = specifier {
            let reference =
                self.reference(ReferenceKind::Import { reexport: false }, specifier, node);
            self.extraction.references.push(reference);
        }
    }

    fn class(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let index = self.define(node, NodeKind::Class, &self.text(name), None);
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
        });
        self.heritage(node, index);
    }

    /// `extends` and `implements` references. They sit outside the class's own scope, so names
    /// resolve from where the class is declared.
    fn heritage(&mut self, class: Node<'_>, index: usize) {
        let Some(heritage) = named_child_of_kind(class, "class_heritage") else {
            return;
        };
        let scope = self.extraction.definitions[index]
            .qualified_name
            .rsplit_once("::")
            .map_or("", |(scope, _)| scope)
            .to_string();
        let mut cursor = heritage.walk();
        for clause in heritage.named_children(&mut cursor) {
            let (kind, names) = match clause.kind() {
                "extends_clause" => (
                    ReferenceKind::Inherits,
                    clause.child_by_field_name("value").into_iter().collect(),
                ),
                "implements_clause" => (ReferenceKind::Implements, named_children(clause)),
                _ => (ReferenceKind::Inherits, vec![clause]),
            };
            for name in names {
                let Some(text) = self.type_path(name) else {
                    continue;
                };
                let mut reference = self.reference(kind, text, name);
                reference.from = Some(index);
                reference.scope = scope.clone();
                self.extraction.references.push(reference);
            }
        }
    }

    /// `Base`, `ns.Base` (as `ns::Base`) or `Generic<T>` (as `Generic`).
    fn type_path(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "identifier" | "type_identifier" => Some(self.text(node)),
            "member_expression" | "nested_type_identifier" => {
                Some(dotted_to_path(&self.text(node)))
            }
            "generic_type" => self.type_path(node.child_by_field_name("name")?),
            _ => None,
        }
    }

    fn named_type(&mut self, node: Node<'_>, kind: NodeKind) {
        if let Some(name) = node.child_by_field_name("name") {
            self.define(node, kind, &self.text(name), None);
        }
    }

    fn namespace(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let index = self.define(
            node,
            NodeKind::Module,
            &dotted_to_path(&self.text(name)),
            None,
        );
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
        });
    }

    fn function(&mut self, node: Node<'_>, kind: NodeKind) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let arity = node.child_by_field_name("parameters").map(parameter_count);
        let index = self.define(node, kind, &self.text(name), arity);
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
        });
    }

    /// `const name = () => ...` and `name = () => ...` in a class body: the function takes the
    /// name of what it is assigned to.
    fn assigned_function(&mut self, node: Node<'_>, kind: NodeKind) {
        let name = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("property"));
        let (Some(name), Some(value)) = (name, node.child_by_field_name("value")) else {
            return;
        };
        let name = self.text(name);
        let arity = value.child_by_field_name("parameters").map(parameter_count);
        let index = self.define(node, kind, &name, arity);
        let definition = &mut self.extraction.definitions[index];
        definition.signature = Some(format!(
            "{name} = {}",
            syntax::signature(value, self.source)
        ));
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
        });
    }

    fn define(&mut self, node: Node<'_>, kind: NodeKind, name: &str, arity: Option<u32>) -> usize {
        let qualified_name = match self.prefix() {
            "" => name.to_string(),
            prefix => format!("{prefix}::{name}"),
        };
        self.extraction.definitions.push(Definition {
            kind,
            name: name.to_string(),
            qualified_name,
            line: syntax::start_line(node),
            end_line: syntax::end_line(node),
            signature: Some(syntax::signature(node, self.source)),
            doc: syntax::leading_doc(
                declaration_statement(node),
                self.source,
                |kind| kind == "comment",
                |_| false,
            ),
            parent: self.enclosing_definition(),
            arity,
        });
        self.extraction.definitions.len() - 1
    }

    fn specifier(&self, node: Node<'_>) -> Option<String> {
        self.string_value(node.child_by_field_name("source")?)
    }

    /// The text inside a string literal; `None` when the node is not a plain string.
    fn string_value(&self, node: Node<'_>) -> Option<String> {
        (node.kind() == "string").then(|| self.text(node).trim_matches(['\'', '"']).to_string())
    }

    fn text(&self, node: Node<'_>) -> String {
        syntax::text(node, self.source).into_owned()
    }
}

/// The statement a definition sits in, where its doc comment is attached: `export class` and
/// `export const f = ...` carry the comment above the `export`.
fn declaration_statement(node: Node<'_>) -> Node<'_> {
    let mut statement = node;
    while let Some(parent) = statement.parent() {
        if !matches!(
            parent.kind(),
            "export_statement" | "lexical_declaration" | "variable_declaration"
        ) {
            break;
        }
        statement = parent;
    }
    statement
}

fn parameter_count(parameters: Node<'_>) -> u32 {
    named_children(parameters)
        .iter()
        .filter(|parameter| parameter.kind() != "comment")
        .count() as u32
}

fn dotted_to_path(text: &str) -> String {
    syntax::without_generics(text).replace('.', "::")
}
