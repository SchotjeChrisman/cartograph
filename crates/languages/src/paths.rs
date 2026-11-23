//! Lexical path helpers for root-relative `/` paths; they never touch the file system.

/// Resolves `.` and `..` segments. A path that climbs above the root keeps its leading `..`.
pub fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

pub fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The file name without its last extension.
pub fn file_stem(path: &str) -> &str {
    let name = file_name(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

pub fn join(dir: &str, relative: &str) -> String {
    if dir.is_empty() {
        normalise(relative)
    } else {
        normalise(&format!("{dir}/{relative}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_dots() {
        assert_eq!(normalise("a/./b/../c.h"), "a/c.h");
        assert_eq!(normalise("../x.h"), "../x.h");
        assert_eq!(join("src/core", "../util/a.h"), "src/util/a.h");
    }

    #[test]
    fn splits_names() {
        assert_eq!(file_stem("a/b/foo.tar.gz"), "foo.tar");
        assert_eq!(parent_dir("foo.h"), "");
    }
}
