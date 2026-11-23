use std::collections::HashMap;

use graph::NodeKind;
use tree_sitter::{Language, Node, Query};

use super::NAMESPACE_SEPARATOR;
use super::laravel;
use crate::model::{Definition, Extraction, Reference, ReferenceKind};
use crate::module::{ExtractError, SourceFile};
use crate::syntax::{self, Capture, has_token, named_child_of_kind, named_children};

/// A class, function or method whose braces are still open at the current capture.
struct Frame {
    end_byte: usize,
    definition: usize,
    /// Parameter types by variable name (with the `$`), for method calls on a typed parameter.
    variable_types: HashMap<String, String>,
}

/// How a call names its callee.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallShape {
    /// `f()`, `Type::member()` and `new Type`: a name that namespaces and imports can settle.
    Path,
    /// `$this->member()`.
    OnThis,
    /// `$value->member()` with `receiver_type` set when the syntax tells it.
    Method,
}

pub(super) struct Extractor<'source> {
    pub(super) path: &'source str,
    pub(super) source: &'source [u8],
    pub(super) extraction: Extraction,
    frames: Vec<Frame>,
    namespace: String,
    namespace_end: Option<usize>,
    /// Property types by class definition index, then property name without the `$`.
    property_types: HashMap<usize, HashMap<String, String>>,
    pub(super) laravel: Option<laravel::Plugin>,
}

pub(super) fn extract(
    language: &Language,
    query: &Query,
    file: &SourceFile<'_>,
) -> Result<Extraction, ExtractError> {
    let tree = syntax::parse(language, file.bytes)?;
    let root = tree.root_node();
    let mut extractor = Extractor {
        path: file.path,
        source: file.bytes,
        extraction: Extraction {
            syntax_errors: syntax::count_syntax_errors(root),
            ..Extraction::default()
        },
        frames: Vec::new(),
        namespace: String::new(),
        namespace_end: None,
        property_types: HashMap::new(),
        laravel: file.context.laravel.then(laravel::Plugin::default),
    };
    for capture in syntax::captures_in_document_order(query, root, file.bytes) {
        extractor.visit(&capture);
    }
    laravel::finish(&mut extractor);
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
        if self
            .namespace_end
            .is_some_and(|end| end <= node.start_byte())
        {
            self.namespace.clear();
            self.namespace_end = None;
        }
        match capture.name {
            "namespace" => self.namespace_definition(node),
            "use" => self.use_declaration(node),
            "class" | "enum" => self.class_like(node, NodeKind::Class),
            "interface" | "trait" => self.class_like(node, NodeKind::Type),
            "function" => self.function(node),
            "method" => self.method(node),
            "trait_use" => self.trait_use(node),
            "property" => self.property(node),
            "call" => self.function_call(node),
            "scoped_call" => self.scoped_call(node),
            "member_call" => self.member_call(node),
            "new" => self.construction(node),
            _ => {}
        }
        laravel::visit(self, capture.name, node);
    }

    pub(super) fn enclosing_definition(&self) -> Option<usize> {
        self.frames.last().map(|frame| frame.definition)
    }

    /// Index of the class, interface or trait around the current position.
    pub(super) fn enclosing_class(&self) -> Option<usize> {
        self.frames
            .iter()
            .rev()
            .map(|frame| frame.definition)
            .find(|&index| {
                matches!(
                    self.extraction.definitions[index].kind,
                    NodeKind::Class | NodeKind::Type
                )
            })
    }

    /// Fully qualified name of the enclosing class, in the form a `\`-prefixed name is written.
    fn enclosing_class_name(&self) -> Option<String> {
        self.enclosing_class().map(|index| {
            format!(
                "{NAMESPACE_SEPARATOR}{}",
                self.extraction.definitions[index].qualified_name
            )
        })
    }

    pub(super) fn reference(&self, kind: ReferenceKind, text: String, node: Node<'_>) -> Reference {
        self.reference_at_line(kind, text, syntax::start_line(node))
    }

    pub(super) fn reference_at_line(
        &self,
        kind: ReferenceKind,
        text: String,
        line: u32,
    ) -> Reference {
        Reference {
            kind,
            text,
            alias: None,
            member: None,
            from: self.enclosing_definition(),
            line,
            arguments: None,
            scope: self.namespace.clone(),
            receiver_type: self.enclosing_class_name(),
            via_self: false,
        }
    }

    pub(super) fn push_reference(&mut self, reference: Reference) {
        self.extraction.references.push(reference);
    }

    pub(super) fn text(&self, node: Node<'_>) -> String {
        syntax::text(node, self.source).into_owned()
    }

    fn namespace_definition(&mut self, node: Node<'_>) {
        self.namespace = node
            .child_by_field_name("name")
            .map(|name| self.text(name))
            .unwrap_or_default();
        self.namespace_end = node.child_by_field_name("body").map(|body| body.end_byte());
    }

    fn use_declaration(&mut self, node: Node<'_>) {
        let declared_with = |token: &str, clause: Option<Node<'_>>| {
            has_token(node, token) || clause.is_some_and(|clause| has_token(clause, token))
        };
        if declared_with("const", None) {
            return;
        }
        let group_prefix =
            named_child_of_kind(node, "namespace_name").map(|prefix| self.text(prefix));
        let clauses = match named_child_of_kind(node, "namespace_use_group") {
            Some(group) => named_children(group),
            None => named_children(node),
        };
        for clause in clauses
            .into_iter()
            .filter(|clause| clause.kind() == "namespace_use_clause")
        {
            if declared_with("const", Some(clause)) {
                continue;
            }
            let member = declared_with("function", Some(clause)).then(|| "function".to_string());
            let alias_node = clause.child_by_field_name("alias");
            let Some(name) = named_children(clause)
                .into_iter()
                .find(|child| Some(child.id()) != alias_node.map(|alias| alias.id()))
            else {
                continue;
            };
            let written = self.text(name);
            let full = match &group_prefix {
                Some(prefix) => format!("{prefix}{NAMESPACE_SEPARATOR}{written}"),
                None => written,
            };
            let full = full.trim_start_matches(NAMESPACE_SEPARATOR).to_string();
            let local = alias_node
                .map_or_else(|| last_segment(&full).to_string(), |alias| self.text(alias));
            let mut reference = self.reference(ReferenceKind::Use { glob: false }, full, node);
            reference.from = None;
            reference.alias = Some(local);
            reference.member = member;
            self.push_reference(reference);
        }
    }

    fn class_like(&mut self, node: Node<'_>, kind: NodeKind) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let qualified_name = self.qualify(&self.text(name));
        let index = self.define(node, kind, &self.text(name), qualified_name, None);
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
            variable_types: HashMap::new(),
        });
        self.heritage(node, index);
    }

    fn heritage(&mut self, class: Node<'_>, index: usize) {
        for (clause_kind, reference_kind) in [
            ("base_clause", ReferenceKind::Inherits),
            ("class_interface_clause", ReferenceKind::Implements),
        ] {
            let Some(clause) = named_child_of_kind(class, clause_kind) else {
                continue;
            };
            for name in named_children(clause) {
                let mut reference = self.reference(reference_kind, self.text(name), name);
                reference.from = Some(index);
                self.push_reference(reference);
            }
        }
    }

    fn trait_use(&mut self, node: Node<'_>) {
        let Some(class) = self.enclosing_class() else {
            return;
        };
        for name in named_children(node)
            .into_iter()
            .filter(|child| matches!(child.kind(), "name" | "qualified_name"))
        {
            let mut reference = self.reference(ReferenceKind::Implements, self.text(name), name);
            reference.from = Some(class);
            self.push_reference(reference);
        }
    }

    fn function(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let qualified_name = self.qualify(&self.text(name));
        self.callable(node, NodeKind::Function, &self.text(name), qualified_name);
    }

    fn method(&mut self, node: Node<'_>) {
        let (Some(name), Some(class)) = (node.child_by_field_name("name"), self.enclosing_class())
        else {
            return;
        };
        let qualified_name = format!(
            "{}::{}",
            self.extraction.definitions[class].qualified_name,
            self.text(name)
        );
        self.callable(node, NodeKind::Method, &self.text(name), qualified_name);
    }

    fn callable(&mut self, node: Node<'_>, kind: NodeKind, name: &str, qualified_name: String) {
        let parameters = node
            .child_by_field_name("parameters")
            .map(named_children)
            .unwrap_or_default();
        let index = self.define(
            node,
            kind,
            name,
            qualified_name,
            Some(parameters.len() as u32),
        );
        let mut variable_types = HashMap::new();
        for parameter in &parameters {
            self.note_parameter_type(parameter, &mut variable_types);
        }
        self.frames.push(Frame {
            end_byte: node.end_byte(),
            definition: index,
            variable_types,
        });
    }

    /// Typed parameters tell the receiver of `$parameter->method()`; promoted ones also type a property.
    fn note_parameter_type(
        &mut self,
        parameter: &Node<'_>,
        variable_types: &mut HashMap<String, String>,
    ) {
        let (Some(type_node), Some(name)) = (
            parameter.child_by_field_name("type"),
            parameter.child_by_field_name("name"),
        ) else {
            return;
        };
        let (Some(type_name), variable) = (self.type_name(type_node), self.text(name)) else {
            return;
        };
        if parameter.kind() == "property_promotion_parameter"
            && let Some(class) = self.enclosing_class()
        {
            self.property_types.entry(class).or_default().insert(
                variable.trim_start_matches('$').to_string(),
                type_name.clone(),
            );
        }
        variable_types.insert(variable, type_name);
    }

    /// The class named by a type declaration; unions and built-in types name none.
    fn type_name(&self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "named_type" => node.named_child(0).map(|name| self.text(name)),
            "optional_type" => self.type_name(node.named_child(0)?),
            _ => None,
        }
    }

    fn property(&mut self, node: Node<'_>) {
        if let (Some(class), Some(type_node)) =
            (self.enclosing_class(), node.child_by_field_name("type"))
            && let Some(type_name) = self.type_name(type_node)
        {
            for element in named_children(node)
                .into_iter()
                .filter(|child| child.kind() == "property_element")
            {
                if let Some(name) = element.child_by_field_name("name") {
                    let name = self.text(name).trim_start_matches('$').to_string();
                    self.property_types
                        .entry(class)
                        .or_default()
                        .insert(name, type_name.clone());
                }
            }
        }
    }

    fn function_call(&mut self, node: Node<'_>) {
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        if matches!(callee.kind(), "name" | "qualified_name") {
            self.push_call(node, self.text(callee), CallShape::Path, None);
        }
    }

    fn scoped_call(&mut self, node: Node<'_>) {
        let (Some(scope), Some(name)) = (
            node.child_by_field_name("scope"),
            node.child_by_field_name("name"),
        ) else {
            return;
        };
        if matches!(scope.kind(), "name" | "qualified_name" | "relative_scope") {
            self.push_call(
                node,
                format!("{}::{}", self.text(scope), self.text(name)),
                CallShape::Path,
                None,
            );
        }
    }

    fn member_call(&mut self, node: Node<'_>) {
        let (Some(object), Some(name)) = (
            node.child_by_field_name("object"),
            node.child_by_field_name("name"),
        ) else {
            return;
        };
        let method = self.text(name);
        if self.text(object) == "$this" {
            self.push_call(node, method, CallShape::OnThis, None);
        } else {
            let receiver_type = self.receiver_type_of(object);
            self.push_call(node, method, CallShape::Method, receiver_type);
        }
    }

    /// The type of `$parameter` or `$this->property` when it was declared.
    fn receiver_type_of(&self, object: Node<'_>) -> Option<String> {
        match object.kind() {
            "variable_name" => self
                .frames
                .iter()
                .rev()
                .find_map(|frame| frame.variable_types.get(&self.text(object)).cloned()),
            "member_access_expression" => {
                let (owner, property) = (
                    object.child_by_field_name("object")?,
                    object.child_by_field_name("name")?,
                );
                if self.text(owner) != "$this" {
                    return None;
                }
                self.property_types
                    .get(&self.enclosing_class()?)?
                    .get(&self.text(property))
                    .cloned()
            }
            _ => None,
        }
    }

    fn construction(&mut self, node: Node<'_>) {
        let Some(class) = named_children(node)
            .into_iter()
            .find(|child| matches!(child.kind(), "name" | "qualified_name" | "relative_scope"))
        else {
            return;
        };
        self.push_call(
            node,
            format!("{}::__construct", self.text(class)),
            CallShape::Path,
            None,
        );
    }

    fn push_call(
        &mut self,
        node: Node<'_>,
        text: String,
        shape: CallShape,
        receiver_type: Option<String>,
    ) {
        let mut reference = self.reference(
            ReferenceKind::Call {
                method: shape != CallShape::Path,
            },
            text,
            node,
        );
        reference.via_self = shape == CallShape::OnThis;
        reference.arguments = node
            .child_by_field_name("arguments")
            .map(|arguments| arguments.named_child_count() as u32);
        if shape == CallShape::Method {
            reference.receiver_type = receiver_type;
        }
        self.push_reference(reference);
    }

    /// `name` inside the current namespace.
    fn qualify(&self, name: &str) -> String {
        match self.namespace.as_str() {
            "" => name.to_string(),
            namespace => format!("{namespace}{NAMESPACE_SEPARATOR}{name}"),
        }
    }

    pub(super) fn define(
        &mut self,
        node: Node<'_>,
        kind: NodeKind,
        name: &str,
        qualified_name: String,
        arity: Option<u32>,
    ) -> usize {
        self.extraction.definitions.push(Definition {
            kind,
            name: name.to_string(),
            qualified_name,
            line: syntax::start_line(node),
            end_line: syntax::end_line(node),
            signature: Some(syntax::signature(node, self.source)),
            doc: syntax::leading_doc(node, self.source, |kind| kind == "comment", |_| false),
            parent: self.enclosing_definition(),
            arity,
        });
        self.extraction.definitions.len() - 1
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit(NAMESPACE_SEPARATOR).next().unwrap_or(name)
}
