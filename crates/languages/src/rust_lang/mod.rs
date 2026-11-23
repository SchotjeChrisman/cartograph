//! Rust. Paths follow the module tree implied by the file layout, so `use` and path calls resolve
//! exactly when the target lives in the tree; method calls on values have no type information
//! and are matched by name, tagged `inferred`.

mod extract;
mod module_path;
mod resolve;

use tree_sitter::{Language, Query};

use crate::model::Extraction;
use crate::module::{ExtractError, LanguageModule, ResolvedEdge, SourceFile};
use crate::project::ProjectView;

pub struct RustModule {
    language: Language,
    query: Query,
}

impl RustModule {
    pub fn new() -> Self {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let query = Query::new(&language, include_str!("../../queries/rust.scm"))
            .unwrap_or_else(|error| panic!("bundled rust query is invalid: {error}"));
        RustModule { language, query }
    }
}

impl Default for RustModule {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageModule for RustModule {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
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
