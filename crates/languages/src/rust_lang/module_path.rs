//! Where a Rust file sits in its crate's module tree, from the file layout alone.

use crate::module::ProjectContext;
use crate::paths::file_stem;

/// Module path segments of `path`, crate name first: `src/a/b.rs` in crate `x` is `x::a::b`.
/// Files outside `src/` (tests, examples, build scripts) are crate roots of their own.
pub(super) fn module_path(path: &str, context: &ProjectContext) -> Vec<String> {
    let Some(package) = context.rust_crate_of(path) else {
        return segments_of(path);
    };
    let inside = path
        .strip_prefix(package.dir.as_str())
        .unwrap_or(path)
        .trim_start_matches('/');
    match inside.strip_prefix("src/") {
        Some("lib.rs" | "main.rs") => vec![package.name.clone()],
        Some(binary) if binary.starts_with("bin/") => segments_of(&binary["bin/".len()..]),
        Some(module_file) => {
            let mut segments = vec![package.name.clone()];
            segments.extend(segments_of(module_file));
            segments
        }
        None => segments_of(without_target_dir(inside)),
    }
}

fn without_target_dir(path: &str) -> &str {
    ["tests/", "examples/", "benches/"]
        .iter()
        .find_map(|dir| path.strip_prefix(dir))
        .unwrap_or(path)
}

/// `a/b.rs` is `[a, b]`; `a/mod.rs` and `a/main.rs` are `[a]`; a lone `x.rs` is `[x]`.
fn segments_of(path: &str) -> Vec<String> {
    let mut segments: Vec<String> = path
        .split('/')
        .map(|part| part.strip_suffix(".rs").unwrap_or(part).to_string())
        .collect();
    if segments.len() > 1 && matches!(segments.last().map(String::as_str), Some("mod" | "main")) {
        segments.pop();
    }
    if segments.is_empty() {
        segments.push(file_stem(path).to_string());
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::RustCrate;

    fn context() -> ProjectContext {
        ProjectContext {
            rust_crates: vec![RustCrate {
                dir: "crates/graph".to_string(),
                name: "graph".to_string(),
            }],
            ..ProjectContext::default()
        }
    }

    #[test]
    fn follows_the_file_layout() {
        let paths = [
            ("crates/graph/src/lib.rs", "graph"),
            ("crates/graph/src/store.rs", "graph::store"),
            ("crates/graph/src/a/mod.rs", "graph::a"),
            ("crates/graph/src/a/b.rs", "graph::a::b"),
            ("crates/graph/src/bin/tool.rs", "tool"),
            ("crates/graph/tests/store.rs", "store"),
            ("crates/graph/tests/common/mod.rs", "common"),
        ];
        for (path, expected) in paths {
            assert_eq!(module_path(path, &context()).join("::"), expected, "{path}");
        }
    }
}
