//! The project-wide symbol table resolvers read. Built from every file's extraction, in path
//! order so resolution is deterministic.

use std::collections::HashMap;

use crate::LanguageModule;
use crate::model::{Definition, Extraction};
use crate::module::ProjectContext;
use crate::module_for_path;
use crate::paths::file_stem;

#[derive(Debug, Clone, Copy)]
pub struct DefinitionRef<'a> {
    pub file: &'a str,
    pub definition: &'a Definition,
}

pub struct ProjectView<'a> {
    pub context: &'a ProjectContext,
    files: HashMap<&'a str, &'a Extraction>,
    by_name: HashMap<&'a str, Vec<DefinitionRef<'a>>>,
    by_qualified: HashMap<&'a str, Vec<DefinitionRef<'a>>>,
    by_stem: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> ProjectView<'a> {
    /// `files` must come in path order.
    pub fn new(
        context: &'a ProjectContext,
        files: impl IntoIterator<Item = (&'a str, &'a Extraction)>,
    ) -> Self {
        let mut view = ProjectView {
            context,
            files: HashMap::new(),
            by_name: HashMap::new(),
            by_qualified: HashMap::new(),
            by_stem: HashMap::new(),
        };
        for (path, extraction) in files {
            view.files.insert(path, extraction);
            view.by_stem.entry(file_stem(path)).or_default().push(path);
            for definition in &extraction.definitions {
                let reference = DefinitionRef {
                    file: path,
                    definition,
                };
                view.by_name
                    .entry(definition.name.as_str())
                    .or_default()
                    .push(reference);
                view.by_qualified
                    .entry(without_overload_suffix(&definition.qualified_name))
                    .or_default()
                    .push(reference);
            }
        }
        view
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn extraction(&self, path: &str) -> Option<&'a Extraction> {
        self.files.get(path).copied()
    }

    /// Definitions whose last name segment is `name`.
    pub fn named(&self, name: &str) -> &[DefinitionRef<'a>] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Definitions with this qualified name; overloads, which the extraction numbers `#2`, `#3`,
    /// all answer to the plain name.
    pub fn qualified(&self, qualified_name: &str) -> &[DefinitionRef<'a>] {
        self.by_qualified
            .get(qualified_name)
            .map_or(&[], Vec::as_slice)
    }

    /// Paths of files whose name without extension is `stem`.
    pub fn files_with_stem(&self, stem: &str) -> &[&'a str] {
        self.by_stem.get(stem).map_or(&[], Vec::as_slice)
    }

    /// Paths of every file, in no particular order.
    pub fn paths(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.files.keys().copied()
    }
}

fn without_overload_suffix(qualified_name: &str) -> &str {
    match qualified_name.rsplit_once('#') {
        Some((plain, number)) if number.chars().all(|digit| digit.is_ascii_digit()) => plain,
        _ => qualified_name,
    }
}

/// One [`ProjectView`] per language family, so a resolver never sees definitions written in a
/// language its own could not call into.
pub struct LanguageViews<'a> {
    by_family: HashMap<&'static str, ProjectView<'a>>,
}

impl<'a> LanguageViews<'a> {
    /// `files` must come in path order.
    pub fn new(
        context: &'a ProjectContext,
        files: impl IntoIterator<Item = (&'a str, &'a Extraction)>,
    ) -> Self {
        let mut grouped: HashMap<&'static str, Vec<(&'a str, &'a Extraction)>> = HashMap::new();
        for (path, extraction) in files {
            if let Some(module) = module_for_path(path) {
                grouped
                    .entry(module.family())
                    .or_default()
                    .push((path, extraction));
            }
        }
        let by_family = grouped
            .into_iter()
            .map(|(family, group)| (family, ProjectView::new(context, group)))
            .collect();
        LanguageViews { by_family }
    }

    /// The view of `module`'s family, or `None` when the tree has no file in that family.
    pub fn for_module(&self, module: &dyn LanguageModule) -> Option<&ProjectView<'a>> {
        self.by_family.get(module.family())
    }
}
