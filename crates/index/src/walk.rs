//! The file walk: the only place that lists directories. It respects `.gitignore` and
//! `.cartographignore`, tags each file with its repository and reports nested graphs; reading
//! file contents is left to the parsing stage.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use graph::GraphDir;
use ignore::WalkBuilder;

use crate::classify::{Classification, classify};
use crate::config::{Config, first_match};
use crate::error::Result;

const GRAPH_DIR_NAME: &str = ".cartograph";
const GIT_ENTRY: &str = ".git";
/// Gitignore syntax, read only by cartograph: the generic `.ignore` is shared with ripgrep and
/// other tools, so excluding something there would hide it from them too.
const IGNORE_FILE_NAME: &str = ".cartographignore";
const GITDIR_PREFIX: &str = "gitdir:";
const WORKTREES_DIR_NAME: &str = "worktrees";
const GIT_CONFIG_FILE: &str = "config";
/// A git config line, with its whitespace removed.
const BARE_SETTING: &str = "bare=true";
/// The repo tag for files whose nearest `.git` is the root itself.
const ROOT_REPO: &str = ".";

/// Whether `directory` is a checkout made by `git worktree add`: its `.git` is a file that points
/// into a repository's `worktrees/` folder. Submodules point into `modules/` and stay indexed,
/// and so do nested clones, whose `.git` is a directory.
pub fn is_linked_worktree(directory: &Path) -> bool {
    git_file_target(directory).is_some_and(|target| {
        target
            .components()
            .any(|part| part.as_os_str() == WORKTREES_DIR_NAME)
    })
}

/// Whether `directory` is the top of a bare-clone layout: its `.git` file points at a bare
/// repository, so the code lives only in the linked worktrees beside it.
pub fn is_bare_clone_root(directory: &Path) -> bool {
    let Some(target) = git_file_target(directory) else {
        return false;
    };
    std::fs::read_to_string(directory.join(target).join(GIT_CONFIG_FILE)).is_ok_and(|config| {
        config
            .lines()
            .any(|line| line.split_whitespace().collect::<String>() == BARE_SETTING)
    })
}

/// The linked worktrees directly inside `directory` that hold a graph, sorted.
pub fn worktrees_with_graph(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|child| {
            is_linked_worktree(child) && GraphDir::of_root(child).database_path().exists()
        })
        .collect();
    found.sort();
    found
}

/// Where a `.git` file points; `None` when `.git` is missing or a directory.
fn git_file_target(directory: &Path) -> Option<PathBuf> {
    let pointer = std::fs::read_to_string(directory.join(GIT_ENTRY)).ok()?;
    Some(PathBuf::from(pointer.strip_prefix(GITDIR_PREFIX)?.trim()))
}

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
    /// Root-relative directories of linked git worktrees; each was skipped, because the same
    /// code checked out on another branch would be indexed a second time.
    pub skipped_worktrees: Vec<String>,
    /// Root-relative paths of every `Cargo.toml`.
    pub cargo_manifests: Vec<String>,
    /// Root-relative paths of every `composer.json`.
    pub composer_manifests: Vec<String>,
    /// Paths the walk could not describe, such as names that are not UTF-8.
    pub warnings: Vec<String>,
}

pub fn scan(root: &Path, config: &Config) -> Result<Scan> {
    let skipped_worktrees = Arc::new(Mutex::new(Vec::new()));
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .add_custom_ignore_filename(IGNORE_FILE_NAME)
        .require_git(false)
        .git_global(false)
        .parents(false)
        .follow_links(false)
        .filter_entry({
            let skipped = Arc::clone(&skipped_worktrees);
            move |entry| {
                let name = entry.file_name();
                if name == GIT_ENTRY || name == GRAPH_DIR_NAME {
                    return false;
                }
                let is_worktree = entry.depth() > 0
                    && entry.file_type().is_some_and(|kind| kind.is_dir())
                    && is_linked_worktree(entry.path());
                if is_worktree {
                    skipped
                        .lock()
                        .expect("no walk callback panics while holding the lock")
                        .push(entry.path().to_path_buf());
                }
                !is_worktree
            }
        });
    let mut scan = Scan {
        files: Vec::new(),
        nested_graphs: Vec::new(),
        skipped_worktrees: Vec::new(),
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
    scan.skipped_worktrees = skipped_worktrees
        .lock()
        .expect("the walk has finished, so no callback holds the lock")
        .iter()
        .filter_map(|path| relative_path(root, path))
        .collect();
    scan.skipped_worktrees.sort();
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
