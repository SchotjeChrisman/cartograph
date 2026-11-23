//! C and C++. Without a compiler there is no real name lookup, so includes are found by path
//! search and calls by name, enclosing scope, argument count and include visibility; every call
//! edge is tagged `inferred` and an ambiguous call stays unresolved. A `compile_commands.json`
//! next to the sources supplies exact include directories.
//!
//! Headers use the C++ grammar: it reads C headers fine and most `.h` files in mixed trees are C++.

mod extract;
mod resolve;

use tree_sitter::{Language, Query};

use crate::model::Extraction;
use crate::module::{ExtractError, LanguageModule, ResolvedEdge, SourceFile};
use crate::project::ProjectView;

pub struct CFamily {
    name: &'static str,
    extensions: &'static [&'static str],
    language: Language,
    query: Query,
    has_namespaces: bool,
}

impl CFamily {
    pub fn c() -> Self {
        Self::new(
            "c",
            &["c"],
            tree_sitter_c::LANGUAGE.into(),
            include_str!("../../queries/c.scm"),
            false,
        )
    }

    pub fn cpp() -> Self {
        Self::new(
            "cpp",
            &[
                "cc", "cpp", "cxx", "c++", "h", "hh", "hpp", "hxx", "inl", "ipp", "tpp",
            ],
            tree_sitter_cpp::LANGUAGE.into(),
            include_str!("../../queries/cpp.scm"),
            true,
        )
    }

    fn new(
        name: &'static str,
        extensions: &'static [&'static str],
        language: Language,
        query_source: &str,
        has_namespaces: bool,
    ) -> Self {
        let query = Query::new(&language, query_source)
            .unwrap_or_else(|error| panic!("bundled {name} query is invalid: {error}"));
        CFamily {
            name,
            extensions,
            language,
            query,
            has_namespaces,
        }
    }
}

impl LanguageModule for CFamily {
    fn name(&self) -> &'static str {
        self.name
    }

    fn family(&self) -> &'static str {
        "c_family"
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ExtractError> {
        extract::extract(&self.language, &self.query, self.has_namespaces, file)
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

/// Extensions of every file the C-family resolver treats as a C or C++ unit; used to find the
/// implementation that goes with a header.
pub(crate) const SOURCE_EXTENSIONS: [&str; 5] = ["c", "cc", "cpp", "cxx", "c++"];
