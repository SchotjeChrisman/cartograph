//! The strict PreToolUse hook: denies Read and Grep until a graph query has oriented the agent,
//! except where the graph cannot help because the files are not parsed.
//!
//! Printing nothing means "no objection"; a denial is the documented JSON on stdout.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use graph::{FileRow, FileStatus, GraphDir, Reader};
use serde_json::{Value, json};

use crate::{Result, orientation};

const DENY_REASON: &str = "Ask the code graph first: call the cartograph `overview` or `find` \
tool (or run `cartograph overview` / `cartograph find <name>`), then read the files it points \
to. Files the graph did not parse stay readable; `coverage` lists them.";

/// What the hook is about to let through or stop.
#[derive(Debug, PartialEq, Eq)]
pub enum Scope {
    /// A Read of one file, root-relative.
    File(String),
    /// A Grep over a directory, root-relative; empty means the whole root.
    Directory(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

/// Whether the graph has nothing to say about `scope`: the file is not parsed or not in the
/// graph, or the directory holds no parsed file.
pub fn graph_cannot_help(files: &[FileRow], scope: &Scope) -> bool {
    match scope {
        Scope::File(path) => files
            .iter()
            .find(|row| &row.path == path)
            .is_none_or(|row| row.status != FileStatus::Parsed),
        Scope::Directory(directory) => !files
            .iter()
            .filter(|row| is_under(&row.path, directory))
            .any(|row| row.status == FileStatus::Parsed),
    }
}

fn is_under(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Reads the hook event from `input` and returns the text to print, if any.
pub fn run(root: &Path, mut input: impl Read) -> Result<Option<String>> {
    let mut text = String::new();
    input.read_to_string(&mut text)?;
    let event: Value = serde_json::from_str(&text)
        .map_err(|error| format!("hook event is not valid JSON: {error}"))?;
    let Some(scope) = scope_of(&event, root) else {
        return Ok(None);
    };
    let dir = GraphDir::of_root(root);
    let decision = decide(&dir, &scope, SystemTime::now())?;
    Ok((decision == Decision::Deny).then(deny_output))
}

fn decide(dir: &GraphDir, scope: &Scope, now: SystemTime) -> Result<Decision> {
    let reader = match Reader::open(dir) {
        Ok(reader) => reader,
        Err(graph::Error::NotIndexed(_)) => return Ok(Decision::Allow),
        Err(error) => return Err(error.into()),
    };
    if orientation::is_recent(dir, now)? || graph_cannot_help(&reader.files()?, scope) {
        return Ok(Decision::Allow);
    }
    Ok(Decision::Deny)
}

fn deny_output() -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": DENY_REASON,
        }
    })
    .to_string()
}

/// The part of the tree a Read or Grep event touches; `None` for any other tool or a path
/// outside the root.
fn scope_of(event: &Value, root: &Path) -> Option<Scope> {
    let input = &event["tool_input"];
    let working_directory = event["cwd"]
        .as_str()
        .map_or_else(|| root.to_path_buf(), PathBuf::from);
    match event["tool_name"].as_str()? {
        "Read" => {
            let path = working_directory.join(input["file_path"].as_str()?);
            relative_to(root, &path).map(Scope::File)
        }
        "Grep" => {
            let path = input["path"]
                .as_str()
                .map_or(working_directory.clone(), |given| {
                    working_directory.join(given)
                });
            let relative = relative_to(root, &path)?;
            Some(if path.is_file() {
                Scope::File(relative)
            } else {
                Scope::Directory(relative)
            })
        }
        _ => None,
    }
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(relative.to_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use graph::FileTags;

    use super::*;

    fn row(path: &str, status: FileStatus) -> FileRow {
        FileRow {
            id: 0,
            path: path.to_string(),
            repo: String::new(),
            language: None,
            content_hash: None,
            size_bytes: 0,
            mtime_ns: None,
            status,
            tags: FileTags::default(),
            component: None,
        }
    }

    #[test]
    fn a_file_is_open_to_read_only_when_the_graph_does_not_cover_it() {
        let files = [
            row("a.rs", FileStatus::Parsed),
            row("b.py", FileStatus::Unparsed),
        ];
        assert!(!graph_cannot_help(&files, &Scope::File("a.rs".into())));
        assert!(graph_cannot_help(&files, &Scope::File("b.py".into())));
        assert!(graph_cannot_help(&files, &Scope::File("README.md".into())));
    }

    #[test]
    fn a_directory_is_open_to_grep_only_when_it_holds_no_parsed_file() {
        let files = [
            row("src/a.rs", FileStatus::Parsed),
            row("scripts/b.py", FileStatus::Unparsed),
            row("srcx/c.py", FileStatus::ParseFailed),
        ];
        assert!(!graph_cannot_help(&files, &Scope::Directory("src".into())));
        assert!(graph_cannot_help(
            &files,
            &Scope::Directory("scripts".into())
        ));
        assert!(graph_cannot_help(&files, &Scope::Directory("srcx".into())));
        assert!(graph_cannot_help(&files, &Scope::Directory("docs".into())));
        assert!(!graph_cannot_help(&files, &Scope::Directory(String::new())));
    }

    #[test]
    fn events_map_to_scopes_inside_the_root() {
        let root = Path::new("/p");
        let read = json!({"tool_name": "Read", "tool_input": {"file_path": "/p/src/a.rs"}});
        assert_eq!(scope_of(&read, root), Some(Scope::File("src/a.rs".into())));
        let outside = json!({"tool_name": "Read", "tool_input": {"file_path": "/etc/hosts"}});
        assert_eq!(scope_of(&outside, root), None);
        let other = json!({"tool_name": "Bash", "tool_input": {}});
        assert_eq!(scope_of(&other, root), None);
    }
}
