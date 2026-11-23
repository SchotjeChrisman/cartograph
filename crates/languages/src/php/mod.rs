//! PHP. Class names resolve through the file's namespace and `use` lines to the project's
//! definitions; calls on `$this`, on typed parameters and on typed properties resolve through the
//! class hierarchy, other method calls by name, tagged `inferred`, and an ambiguous name stays
//! unresolved. The Laravel plugin in [`laravel`] adds routes, Eloquent tables, container bindings,
//! events, config keys and Blade views when the project depends on `laravel/framework`.

mod extract;
mod laravel;
mod resolve;

use tree_sitter::{Language, Query};

use crate::model::Extraction;
use crate::module::{ExtractError, LanguageModule, ResolvedEdge, SourceFile};
use crate::project::ProjectView;

/// Separates namespace segments in PHP names; the graph keeps them as written.
const NAMESPACE_SEPARATOR: char = '\\';

pub struct PhpModule {
    language: Language,
    query: Query,
}

impl PhpModule {
    pub fn new() -> Self {
        let language: Language = tree_sitter_php::LANGUAGE_PHP.into();
        let query = Query::new(&language, include_str!("../../queries/php.scm"))
            .unwrap_or_else(|error| panic!("bundled php query is invalid: {error}"));
        PhpModule { language, query }
    }
}

impl Default for PhpModule {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageModule for PhpModule {
    fn name(&self) -> &'static str {
        "php"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
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
