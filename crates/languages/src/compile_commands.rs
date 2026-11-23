//! Reading `compile_commands.json` for the include directories each translation unit is built
//! with. Defines and overload sets would need a preprocessor and a type checker, so they stay
//! out of reach of this heuristic indexer.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths::{join, normalise};

/// Include directories per file, as root-relative paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CompileCommands {
    include_dirs: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Entry {
    directory: String,
    file: String,
    command: Option<String>,
    arguments: Option<Vec<String>>,
}

const INCLUDE_FLAGS: [&str; 3] = ["-I", "-isystem", "-iquote"];

impl CompileCommands {
    /// Parses the JSON text; entries pointing outside `root` are dropped.
    pub fn parse(json: &str, root: &Path) -> Result<Self, serde_json::Error> {
        let entries: Vec<Entry> = serde_json::from_str(json)?;
        let root = root.to_string_lossy();
        let mut include_dirs = BTreeMap::new();
        for entry in entries {
            let Some(directory) = relative_to_root(&entry.directory, "", &root) else {
                continue;
            };
            let Some(file) = relative_to_root(&entry.file, &directory, &root) else {
                continue;
            };
            let arguments = entry.arguments.unwrap_or_else(|| {
                entry
                    .command
                    .iter()
                    .flat_map(|command| command.split_whitespace().map(str::to_string))
                    .collect()
            });
            let dirs = include_dirs_of(&arguments, &directory, &root);
            include_dirs.insert(file, dirs);
        }
        Ok(CompileCommands { include_dirs })
    }

    /// Include directories of the build command for `file`; empty when it has none.
    pub fn include_dirs_of(&self, file: &str) -> &[String] {
        self.include_dirs.get(file).map_or(&[], Vec::as_slice)
    }
}

fn include_dirs_of(arguments: &[String], directory: &str, root: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let value = INCLUDE_FLAGS
            .iter()
            .find_map(|flag| match argument.strip_prefix(flag) {
                Some("") => arguments.next().map(String::as_str),
                Some(attached) => Some(attached),
                None => None,
            });
        if let Some(dir) = value.and_then(|value| relative_to_root(value, directory, root)) {
            dirs.push(dir);
        }
    }
    dirs
}

/// `path` as a root-relative path, resolving a relative one against `base` (itself
/// root-relative). `None` when the result lies outside the root.
fn relative_to_root(path: &str, base: &str, root: &str) -> Option<String> {
    let root = root.trim_end_matches('/');
    let relative = match path.strip_prefix(root) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            rest.trim_start_matches('/').to_string()
        }
        _ if path.starts_with('/') => return None,
        _ => join(base, path),
    };
    let normalised = normalise(&relative);
    (!normalised.starts_with("..")).then_some(normalised)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_include_dirs_from_command_and_arguments() {
        let json = r#"[
            {"directory": "/repo/build", "file": "../src/a.cpp",
             "command": "c++ -I../include -isystem /repo/third -I /usr/include -DX=1 -c ../src/a.cpp"},
            {"directory": "/repo", "file": "src/b.cpp", "arguments": ["cc", "-Iinc", "-iquote", "src"]}
        ]"#;
        let commands = CompileCommands::parse(json, Path::new("/repo")).unwrap();
        assert_eq!(commands.include_dirs_of("src/a.cpp"), ["include", "third"]);
        assert_eq!(commands.include_dirs_of("src/b.cpp"), ["inc", "src"]);
        assert!(commands.include_dirs_of("src/none.cpp").is_empty());
    }
}
