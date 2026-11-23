//! The interface every language implements. TypeScript and PHP modules plug in the same way:
//! implement [`LanguageModule`], add the module to `modules()` in `lib.rs`.
//!
//! A module has two jobs, kept apart so the first can be cached by content hash:
//! 1. [`LanguageModule::extract`] turns one file's bytes into definitions and unresolved
//!    references. It reads nothing else.
//! 2. [`LanguageModule::resolve`] turns those references into edges using the whole project.

use graph::{EdgeKind, NodeKind, Provenance, SymbolId};
use serde::Serialize;

use crate::compile_commands::CompileCommands;
use crate::model::Extraction;
use crate::project::ProjectView;

/// One file as the walker found it.
pub struct SourceFile<'a> {
    /// Path relative to the graph root, with `/` separators.
    pub path: &'a str,
    pub bytes: &'a [u8],
    pub context: &'a ProjectContext,
}

/// Facts about the whole tree that the walker reads from manifests; both parts change which
/// facts a file yields, so the indexer re-extracts everything when they change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProjectContext {
    pub rust_crates: Vec<RustCrate>,
    pub compile_commands: CompileCommands,
    /// True when a `composer.json` requires `laravel/framework`; it switches on the Laravel plugin.
    pub laravel: bool,
}

/// A Cargo package found in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RustCrate {
    /// Directory of the package's `Cargo.toml`, relative to the root; empty for the root itself.
    pub dir: String,
    /// The crate name as Rust code spells it (dashes turned into underscores).
    pub name: String,
}

impl ProjectContext {
    /// The package whose directory is the deepest ancestor of `path`.
    pub fn rust_crate_of(&self, path: &str) -> Option<&RustCrate> {
        self.rust_crates
            .iter()
            .filter(|package| {
                package.dir.is_empty()
                    || path
                        .strip_prefix(package.dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|package| package.dir.len())
    }
}

#[derive(Debug)]
pub enum ExtractError {
    Grammar(String),
    /// The parser gave up, for example on a timeout.
    NoTree,
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Grammar(message) => {
                write!(formatter, "grammar failed to load: {message}")
            }
            ExtractError::NoTree => formatter.write_str("parser produced no syntax tree"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// A language the indexer can parse.
pub trait LanguageModule: Send + Sync {
    /// The name stored in the files table and shown by coverage.
    fn name(&self) -> &'static str;

    /// Languages of one family resolve into each other (C and C++, JavaScript and TypeScript);
    /// a name never resolves across families.
    fn family(&self) -> &'static str {
        self.name()
    }

    /// Lowercase file extensions without the dot.
    fn extensions(&self) -> &'static [&'static str];

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ExtractError>;

    /// Edges for the references of one file, given every file's extraction.
    /// Must depend only on its arguments so incremental and full indexing agree.
    fn resolve(
        &self,
        path: &str,
        extraction: &Extraction,
        project: &ProjectView<'_>,
    ) -> Vec<ResolvedEdge>;
}

/// One end of an edge, named by what identifies it rather than by id so the caller decides
/// how ids are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Definition {
        file: String,
        kind: NodeKind,
        qualified_name: String,
    },
    File(String),
    /// A name that could not be pinned to one definition.
    Unresolved(String),
}

impl Endpoint {
    pub fn symbol_id(&self) -> SymbolId {
        match self {
            Endpoint::Definition {
                file,
                kind,
                qualified_name,
            } => SymbolId::derive(*kind, qualified_name, file),
            Endpoint::File(path) => SymbolId::derive(NodeKind::File, path, path),
            Endpoint::Unresolved(name) => SymbolId::derive(NodeKind::Unresolved, name, ""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEdge {
    pub src: Endpoint,
    pub dst: Endpoint,
    pub kind: EdgeKind,
    pub provenance: Provenance,
    pub line: u32,
    /// Required when `provenance` is inferred; also says why a name stayed unresolved.
    pub reason: Option<String>,
}

/// The `contains` edges every language shares: file or enclosing definition to definition.
pub fn containment_edges(path: &str, extraction: &Extraction) -> Vec<ResolvedEdge> {
    let endpoint_of = |index: usize| Endpoint::Definition {
        file: path.to_string(),
        kind: extraction.definitions[index].kind,
        qualified_name: extraction.definitions[index].qualified_name.clone(),
    };
    extraction
        .definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| ResolvedEdge {
            src: definition
                .parent
                .map_or_else(|| Endpoint::File(path.to_string()), endpoint_of),
            dst: endpoint_of(index),
            kind: EdgeKind::Contains,
            provenance: Provenance::Extracted,
            line: definition.line,
            reason: None,
        })
        .collect()
}

/// Where a reference originates: its enclosing definition, or the file.
pub fn origin_endpoint(path: &str, extraction: &Extraction, from: Option<usize>) -> Endpoint {
    match from {
        Some(index) => Endpoint::Definition {
            file: path.to_string(),
            kind: extraction.definitions[index].kind,
            qualified_name: extraction.definitions[index].qualified_name.clone(),
        },
        None => Endpoint::File(path.to_string()),
    }
}
