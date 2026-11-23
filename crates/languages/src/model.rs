//! What a language module learns from one file, before anything is resolved across files.

use graph::NodeKind;
use serde::{Deserialize, Serialize};

/// A symbol defined in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(with = "node_kind_text")]
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub doc: Option<String>,
    /// Index of the definition that lexically contains this one; `None` means the file does.
    pub parent: Option<usize>,
    /// Parameter count, used to pick between overloads.
    pub arity: Option<u32>,
}

/// What a reference points at, as far as the syntax says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceKind {
    /// `#include`; `system` is true for the angle-bracket form.
    Include {
        system: bool,
    },
    /// Rust or PHP `use`; `glob` is true for `use path::*`.
    Use {
        glob: bool,
    },
    /// JavaScript or TypeScript `import`, `require` and `export ... from`: `text` is the module
    /// specifier, `alias` the local name it binds, `member` the export taken (`None` binds the
    /// whole module). `reexport` marks `export ... from`, which binds nothing locally.
    Import {
        reexport: bool,
    },
    /// JavaScript or TypeScript `export default`: `text` is the local name that is exported.
    DefaultExport,
    /// Rust `mod name;`.
    ModuleDeclaration,
    /// A call; `method` is true when the callee is a member of some receiver expression.
    Call {
        method: bool,
    },
    Inherits,
    /// `text` is the trait, `receiver_type` the implementing type.
    Implements,
    /// Wiring a framework plugin recognised in the code.
    Framework(FrameworkLink),
}

/// The relations a framework plugin reads out of code. `from` is where the wiring sits unless
/// the variant says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameworkLink {
    /// `from` is the route; `text` the handler class and `member` its action. A resource route
    /// names actions the controller may leave out, so a missing one is `optional`, not an error.
    RouteHandler {
        optional: bool,
    },
    /// `from` is a model class; `text` is its table, empty for the conventional name.
    ModelTable,
    TableAccess {
        write: bool,
    },
    ConfigAccess {
        write: bool,
    },
    /// `text` is the event the code fires.
    HookFired,
    /// `text` is the event the code (a listener method) handles.
    HookHandled,
    /// `text` is a listener class and `member` the event class it is registered for.
    Listener,
    /// A container binding: `text` is the abstract type and `member` the concrete class.
    Binding,
    /// `text` is a view name such as `users.index`.
    View,
}

/// A mention of something that may be defined elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub kind: ReferenceKind,
    /// The name or path as written, generics removed.
    pub text: String,
    /// The local name a `use` binds.
    pub alias: Option<String>,
    /// A second name the reference carries: the export an import binds, the action of a route
    /// handler, the concrete class of a container binding, the event a listener is for.
    #[serde(default)]
    pub member: Option<String>,
    /// Index of the definition the reference sits in; `None` means file level.
    pub from: Option<usize>,
    pub line: u32,
    /// Argument count of a call.
    pub arguments: Option<u32>,
    /// Qualified name of the namespace or module the reference sits in; names resolve outward from it.
    pub scope: String,
    /// The type the enclosing `impl` or class is for; for a PHP method call, the receiver's type
    /// as written (a leading `\` marks a fully qualified name) when the syntax tells it.
    pub receiver_type: Option<String>,
    /// True for a method call on `self`.
    pub via_self: bool,
}

/// Everything one file contributes before cross-file resolution. Pure function of the file's
/// bytes and the project context, so it can be cached by content hash.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    /// Syntax error and missing-token nodes the parser recovered from.
    pub syntax_errors: u32,
}

impl Extraction {
    /// What other files can see of this one: its definitions without signatures and docs, and
    /// every reference but calls. Resolvers read another file's include chain, imports and
    /// inheritance, never its calls, and calls are most of an extraction, so a view of every file
    /// built from these stays small enough to hold a whole tree.
    pub fn visible_to_other_files(&self) -> Extraction {
        Extraction {
            definitions: self
                .definitions
                .iter()
                .map(|definition| Definition {
                    signature: None,
                    doc: None,
                    ..definition.clone()
                })
                .collect(),
            references: self
                .references
                .iter()
                .filter(|reference| !matches!(reference.kind, ReferenceKind::Call { .. }))
                .cloned()
                .collect(),
            syntax_errors: self.syntax_errors,
        }
    }

    /// Makes qualified names unique within the file, which the graph requires: later duplicates
    /// (overloads, trait impls with the same method name, `#ifdef` variants) get `#2`, `#3`, ...
    pub fn disambiguate_duplicates(&mut self) {
        let mut seen = std::collections::HashMap::<(NodeKind, String), u32>::new();
        for definition in &mut self.definitions {
            let key = (definition.kind, definition.qualified_name.clone());
            let occurrence = seen.entry(key).or_insert(0);
            *occurrence += 1;
            if *occurrence > 1 {
                definition.qualified_name = format!("{}#{occurrence}", definition.qualified_name);
            }
        }
    }

    /// Hangs methods under the class or type of the same file they belong to, such as an
    /// out-of-line `Foo::bar` or a Rust `impl Foo`, so `contains` edges show membership.
    pub fn attach_members_to_types(&mut self) {
        for index in 0..self.definitions.len() {
            let method = &self.definitions[index];
            if method.kind != NodeKind::Method || method.parent.is_some() {
                continue;
            }
            let owner = method
                .qualified_name
                .rsplit_once("::")
                .map(|(scope, _)| scope);
            let found = self.definitions.iter().position(|candidate| {
                matches!(candidate.kind, NodeKind::Class | NodeKind::Type)
                    && Some(candidate.qualified_name.as_str()) == owner
            });
            self.definitions[index].parent = found;
        }
    }
}

/// Serde adapter storing a [`NodeKind`] as its text name, since the graph's enum is only `Serialize`.
mod node_kind_text {
    use graph::NodeKind;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(kind: &NodeKind, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(kind.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NodeKind, D::Error> {
        let text = String::deserialize(deserializer)?;
        NodeKind::parse(&text).ok_or_else(|| D::Error::custom(format!("unknown node kind {text}")))
    }
}
