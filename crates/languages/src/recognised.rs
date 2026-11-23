//! Source languages the indexer knows by extension but has no module for. Their files are
//! recorded as unparsed so `coverage` shows what the graph does not see. Files with any other
//! extension (docs, images, data) are not source code and are not recorded at all.

const KNOWN_UNSUPPORTED: &[(&str, &[&str])] = &[
    ("python", &["py", "pyi"]),
    ("go", &["go"]),
    ("java", &["java"]),
    ("kotlin", &["kt", "kts"]),
    ("swift", &["swift"]),
    ("ruby", &["rb"]),
    ("csharp", &["cs"]),
    ("objective-c", &["m", "mm"]),
    ("shell", &["sh", "bash", "zsh"]),
    ("webidl", &["webidl", "idl"]),
    ("assembly", &["s", "asm"]),
];

/// The language of a source file no module handles, by extension.
pub fn unsupported_language(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    KNOWN_UNSUPPORTED
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        .map(|(language, _)| *language)
}
