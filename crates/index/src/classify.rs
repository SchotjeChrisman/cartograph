//! Deciding, from a path alone, what the indexer does with a file; and tagging files whose
//! path or header marks them as vendored or generated.

use graph::FileTags;
use languages::LanguageModule;

use crate::config::{Config, first_match};

/// How many bytes at the top of a file are searched for a generated-code marker.
const HEADER_BYTES: usize = 1024;

pub enum Classification {
    /// Never read; the pattern that matched is shown by coverage.
    Secret {
        pattern: String,
    },
    Parseable(&'static dyn LanguageModule),
    /// Source code in a language without a module yet.
    Unsupported(&'static str),
    /// A `.cartograph/` directory below the root; it belongs to some other graph.
    NestedGraph,
    /// A `links.yaml` of hand-declared edges; read as text, not parsed as source.
    Declarations,
}

/// `None` for files that are not source code and not secrets: they are left out of the graph.
pub fn classify(path: &str, config: &Config) -> Option<Classification> {
    if let Some(pattern) = first_match(&config.secrets, path) {
        return Some(Classification::Secret {
            pattern: pattern.to_string(),
        });
    }
    if languages::file_name(path) == crate::links::LINKS_FILE {
        return Some(Classification::Declarations);
    }
    if let Some(module) = languages::module_for_path(path) {
        return Some(Classification::Parseable(module));
    }
    languages::unsupported_language(path).map(Classification::Unsupported)
}

/// Tags from the path and, when `header` is given, the file's first bytes.
pub fn tags_for(path: &str, header: Option<&[u8]>, config: &Config) -> FileTags {
    let marked_generated = header.is_some_and(|bytes| {
        let head = &bytes[..bytes.len().min(HEADER_BYTES)];
        let head = String::from_utf8_lossy(head).to_lowercase();
        config
            .generated_markers
            .iter()
            .any(|marker| head.contains(marker.as_str()))
    });
    FileTags {
        vendored: first_match(&config.vendored, path).is_some(),
        generated: marked_generated || first_match(&config.generated, path).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_win_over_language() {
        let config = Config::default();
        assert!(matches!(
            classify("a/.env", &config),
            Some(Classification::Secret { .. })
        ));
        assert!(matches!(
            classify("a/main.rs", &config),
            Some(Classification::Parseable(_))
        ));
        assert!(matches!(
            classify("a/x.py", &config),
            Some(Classification::Unsupported("python"))
        ));
        assert!(classify("README.md", &config).is_none());
    }

    #[test]
    fn tags_come_from_path_and_header() {
        let config = Config::default();
        let tags = tags_for("third_party/x.c", Some(b"// DO NOT EDIT\nint x;"), &config);
        assert!(tags.vendored && tags.generated);
        assert_eq!(
            tags_for("src/a.c", Some(b"int x;"), &config),
            FileTags::default()
        );
    }
}
