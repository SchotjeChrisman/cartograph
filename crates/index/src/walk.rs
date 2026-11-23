//! The file walk: the only place that lists directories. It respects `.gitignore` and
//! `.cartographignore`, tags each file with its repository and reports nested graphs; reading
//! file contents is left to the parsing stage.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::classify::{Classification, classify};
use crate::config::{Config, first_match};
use crate::error::Result;

const GRAPH_DIR_NAME: &str = ".cartograph";
const GIT_ENTRY: &str = ".git";
/// Gitignore syntax, read only by cartograph: the generic `.ignore` is shared with ripgrep and
/// other tools, so excluding something there would hide it from them too.
const IGNORE_FILE_NAME: &str = ".cartographignore";
/// The repo tag for files whose nearest `.git` is the root itself.
const ROOT_REPO: &str = ".";

pub struct ScannedFile {
    /// Root-relative, `/`-separated.
    pub path: String,
    pub absolute: PathBuf,
    /// Root-relative directory holding the nearest `.git`; empty when there is none.
    pub repo: String,
    pub classification: Classification,
}

pub struct Scan {
    /// Sorted by path.
    pub files: Vec<ScannedFile>,
    /// `.cartograph` directories below the root; each was skipped.
    pub nested_graphs: Vec<ScannedFile>,
    /// Root-relative paths of every `Cargo.toml`.
    pub cargo_manifests: Vec<String>,
    /// Root-relative paths of every `composer.json`.
    pub composer_manifests: Vec<String>,
    /// Paths the walk could not describe, such as names that are not UTF-8.
    pub warnings: Vec<String>,
}

pub fn scan(root: &Path, config: &Config) -> Result<Scan> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .add_custom_ignore_filename(IGNORE_FILE_NAME)
        .require_git(false)
        .git_global(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name();
            name != GIT_ENTRY && name != GRAPH_DIR_NAME
        });
    let mut scan = Scan {
        files: Vec::new(),
        nested_graphs: Vec::new(),
        cargo_manifests: Vec::new(),
        composer_manifests: Vec::new(),
        warnings: Vec::new(),
    };
    let mut repo_of_dir: HashMap<PathBuf, String> = HashMap::new();
    // The walk is sequential on purpose: repo tags flow from parent directories to children.
    for entry in builder.build() {
        let entry = entry?;
        let Some(relative) = relative_path(root, entry.path()) else {
            scan.warnings.push(format!(
                "{}: path is not valid UTF-8, skipped",
                entry.path().display()
            ));
            continue;
        };
        let file_type = entry.file_type();
        if file_type.is_some_and(|kind| kind.is_dir()) {
            let parent_repo = entry
                .path()
                .parent()
                .and_then(|parent| repo_of_dir.get(parent))
                .cloned()
                .unwrap_or_default();
            let repo = if entry.path().join(GIT_ENTRY).exists() {
                if relative.is_empty() {
                    ROOT_REPO.to_string()
                } else {
                    relative.clone()
                }
            } else {
                parent_repo
            };
            if !relative.is_empty() && entry.path().join(GRAPH_DIR_NAME).is_dir() {
                scan.nested_graphs.push(ScannedFile {
                    path: format!("{relative}/{GRAPH_DIR_NAME}"),
                    absolute: entry.path().join(GRAPH_DIR_NAME),
                    repo: repo.clone(),
                    classification: Classification::NestedGraph,
                });
            }
            repo_of_dir.insert(entry.path().to_path_buf(), repo);
            continue;
        }
        if !file_type.is_some_and(|kind| kind.is_file()) {
            continue;
        }
        match relative.rsplit('/').next() {
            Some("Cargo.toml") => scan.cargo_manifests.push(relative.clone()),
            Some("composer.json") if first_match(&config.vendored, &relative).is_none() => {
                scan.composer_manifests.push(relative.clone());
            }
            _ => {}
        }
        let Some(classification) = classify(&relative, config) else {
            continue;
        };
        let repo = entry
            .path()
            .parent()
            .and_then(|parent| repo_of_dir.get(parent))
            .cloned()
            .unwrap_or_default();
        scan.files.push(ScannedFile {
            path: relative,
            absolute: entry.path().to_path_buf(),
            repo,
            classification,
        });
    }
    scan.files.sort_by(|left, right| left.path.cmp(&right.path));
    scan.nested_graphs
        .sort_by(|left, right| left.path.cmp(&right.path));
    scan.cargo_manifests.sort();
    scan.composer_manifests.sort();
    Ok(scan)
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect();
    Some(parts?.join("/"))
}
