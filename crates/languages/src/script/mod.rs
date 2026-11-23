//! JavaScript and TypeScript (and TSX/JSX). Relative imports resolve to files exactly; a call
//! resolves through the file's own scopes and imports. Method calls on values have no type
//! information and are matched by name, tagged `inferred`; an ambiguous name stays unresolved.
//! Bare package specifiers and path aliases (`tsconfig.json` paths) are treated as external.

mod extract;
mod resolve;

use tree_sitter::{Language, Query};

use crate::model::Extraction;
use crate::module::{ExtractError, LanguageModule, ResolvedEdge, SourceFile};
use crate::project::ProjectView;

/// Extensions the resolver tries, in order, when an import names a file without one.
pub(crate) const SCRIPT_EXTENSIONS: [&str; 8] =
    ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

pub struct ScriptModule {
    name: &'static str,
    extensions: &'static [&'static str],
    language: Language,
    query: Query,
}

impl ScriptModule {
    pub fn javascript() -> Self {
        let query = format!(
            "{}\n{}",
            include_str!("../../queries/javascript.scm"),
            include_str!("../../queries/jsx.scm")
        );
        Self::new(
            "javascript",
            &["js", "jsx", "mjs", "cjs"],
            tree_sitter_javascript::LANGUAGE.into(),
            &query,
        )
    }

    pub fn typescript() -> Self {
        Self::new(
            "typescript",
            &["ts", "mts", "cts"],
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            include_str!("../../queries/typescript.scm"),
        )
    }

    pub fn tsx() -> Self {
        let query = format!(
            "{}\n{}",
            include_str!("../../queries/typescript.scm"),
            include_str!("../../queries/jsx.scm")
        );
        Self::new(
            "tsx",
            &["tsx"],
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &query,
        )
    }

    fn new(
        name: &'static str,
        extensions: &'static [&'static str],
        language: Language,
        query_source: &str,
    ) -> Self {
        let query = Query::new(&language, query_source)
            .unwrap_or_else(|error| panic!("bundled {name} query is invalid: {error}"));
        ScriptModule {
            name,
            extensions,
            language,
            query,
        }
    }
}

impl LanguageModule for ScriptModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn family(&self) -> &'static str {
        "script"
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ExtractError> {
        extract::extract(&self.language, &self.query, file)
    }

    fn resolve(
        &self,
        path: &str,
        extraction: &Extraction,
        project: &ProjectView<'_>,
    ) -> Vec<ResolvedEdge> {
        resolve::resolve(path, extraction, project)
    }
}
