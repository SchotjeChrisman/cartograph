//! Language modules: one per language, each a tree-sitter grammar, a `.scm` query file naming
//! the syntax of interest, and a small Rust part that names symbols and resolves references.
//! The shared interface is [`LanguageModule`]; this crate holds no I/O.
//!
//! Adding a language: write the module, then list it in [`modules`].

mod c_family;
mod compile_commands;
mod model;
mod module;
mod paths;
mod php;
mod project;
mod recognised;
mod rust_lang;
mod script;
mod syntax;

use std::sync::LazyLock;

pub use compile_commands::CompileCommands;
pub use model::{Definition, Extraction, FrameworkLink, Reference, ReferenceKind};
pub use module::{
    Endpoint, ExtractError, LanguageModule, ProjectContext, ResolvedEdge, RustCrate, SourceFile,
    containment_edges,
};
pub use paths::{file_name, file_stem, parent_dir};
pub use project::{DefinitionRef, LanguageViews, ProjectView};
pub use recognised::unsupported_language;

static MODULES: LazyLock<Vec<Box<dyn LanguageModule>>> = LazyLock::new(|| {
    vec![
        Box::new(c_family::CFamily::c()),
        Box::new(c_family::CFamily::cpp()),
        Box::new(rust_lang::RustModule::new()),
        Box::new(script::ScriptModule::javascript()),
        Box::new(script::ScriptModule::typescript()),
        Box::new(script::ScriptModule::tsx()),
        Box::new(php::PhpModule::new()),
    ]
});

/// Every compiled-in language module.
pub fn modules() -> &'static [Box<dyn LanguageModule>] {
    &MODULES
}

/// The module that parses `path`, chosen by extension.
pub fn module_for_path(path: &str) -> Option<&'static dyn LanguageModule> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    modules()
        .iter()
        .find(|module| module.extensions().contains(&extension.as_str()))
        .map(|module| module.as_ref())
}

/// The module with this name, as stored in the files table.
pub fn module_named(name: &str) -> Option<&'static dyn LanguageModule> {
    modules()
        .iter()
        .find(|module| module.name() == name)
        .map(|module| module.as_ref())
}
