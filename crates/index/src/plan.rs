//! Deciding which unchanged files must be resolved again because something they refer to moved.
//! Pure: it only compares names.
//!
//! A file's edges depend on the definitions its references name, and, for includes, on which
//! files exist and what they include in turn. So a change is summarised as a set of keys (the
//! names of definitions that appeared, vanished or changed shape, and the stems of files that
//! came, went or changed their includes), and every
//! file with a reference to one of those keys is affected. An affected file that matched
//! through an include is itself a changed link in the include chain, so its stem becomes a key
//! too, until nothing new is reached.
//!
//! ponytail: a changed name wakes every reference to that name, even ones a scope lookup would
//! settle regardless. Record which references fell back to by-name lookup to narrow this.

use std::collections::{HashMap, HashSet};

use graph::NodeKind;
use languages::{
    Definition, Extraction, FrameworkLink, Reference, ReferenceKind, file_name, file_stem,
};

/// One file that was added, removed or rewritten, with what it held before and after.
pub struct Change<'a> {
    pub path: &'a str,
    pub old: Option<&'a Extraction>,
    pub new: Option<&'a Extraction>,
}

type Signatures<'a> = HashMap<(NodeKind, &'a str, Option<u32>), &'a str>;

fn signatures(extraction: Option<&Extraction>) -> Signatures<'_> {
    extraction
        .into_iter()
        .flat_map(|extraction| &extraction.definitions)
        .map(|definition| (resolution_signature(definition), definition.name.as_str()))
        .collect()
}

/// What resolution reads from a definition; anything else about it may change freely.
fn resolution_signature(definition: &Definition) -> (NodeKind, &str, Option<u32>) {
    (
        definition.kind,
        definition.qualified_name.as_str(),
        definition.arity,
    )
}

/// What a file says about its own place among files: the headers it includes, the modules it
/// imports or re-exports, and what it exports by default.
fn file_links(extraction: &Extraction) -> Vec<String> {
    let mut links: Vec<String> = extraction
        .references
        .iter()
        .filter(|reference| {
            matches!(
                reference.kind,
                ReferenceKind::Include { .. }
                    | ReferenceKind::Import { .. }
                    | ReferenceKind::DefaultExport
            )
        })
        .map(|reference| {
            format!(
                "{:?} {} {:?}",
                reference.kind, reference.text, reference.member
            )
        })
        .collect();
    links.sort_unstable();
    links
}

/// Keys describing what other files can notice about the changes: the names of definitions that
/// appeared, vanished or changed shape, and, for files that came or went or changed their
/// includes, the file's stem and directory name.
pub fn changed_keys(changes: &[Change<'_>]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for change in changes {
        let path_changed = match (change.old, change.new) {
            (Some(old), Some(new)) => file_links(old) != file_links(new),
            (None, None) => continue,
            _ => true,
        };
        if path_changed {
            keys.insert(file_stem(change.path).to_string());
            if let Some(directory) = change.path.rsplit('/').nth(1) {
                keys.insert(directory.to_string());
            }
        }
        let (before, after) = (signatures(change.old), signatures(change.new));
        let moved = before
            .iter()
            .filter(|(signature, _)| !after.contains_key(*signature))
            .chain(
                after
                    .iter()
                    .filter(|(signature, _)| !before.contains_key(*signature)),
            );
        keys.extend(moved.map(|(_, name)| name.to_string()));
    }
    keys
}

/// Methods a registered listener runs without naming them, so they are keys of the registration.
const LISTENER_HANDLER_NAMES: [&str; 2] = ["handle", "__invoke"];

/// The parts of a written name that a definition could be named by: `a::b`, `A\B` and `a.b`.
fn name_parts(text: &str) -> impl Iterator<Item = &str> {
    text.split("::").flat_map(|part| part.split('\\'))
}

/// A key a reference depends on, and whether it came from an include or import.
fn reference_keys(reference: &Reference) -> Vec<(String, bool)> {
    let mut keys: Vec<(String, bool)> = match reference.kind {
        ReferenceKind::Include { .. } | ReferenceKind::Import { .. } => {
            vec![(file_stem(file_name(&reference.text)).to_string(), true)]
        }
        ReferenceKind::Implements => name_parts(&reference.text)
            .chain(
                reference
                    .receiver_type
                    .as_deref()
                    .and_then(|text| text.rsplit("::").next()),
            )
            .map(|segment| (segment.to_string(), false))
            .collect(),
        ReferenceKind::Framework(FrameworkLink::View) => {
            let template = reference.text.rsplit('.').next().unwrap_or_default();
            vec![
                (format!("{template}.blade"), false),
                (template.to_string(), false),
            ]
        }
        ReferenceKind::Framework(FrameworkLink::Listener) => name_parts(&reference.text)
            .chain(LISTENER_HANDLER_NAMES)
            .map(|segment| (segment.to_string(), false))
            .collect(),
        _ => name_parts(&reference.text)
            .map(|segment| (segment.to_string(), false))
            .collect(),
    };
    keys.extend(
        reference
            .member
            .iter()
            .flat_map(|member| name_parts(member))
            .map(|segment| (segment.to_string(), false)),
    );
    keys
}

/// Grows `keys` along include chains: a file that refers to a changed key through an include or
/// import is itself a changed link, so its stem becomes a key too, until nothing new is
/// reached. Returns the grown keys and the files met on the way.
pub fn follow_includes<'a>(
    mut keys: HashSet<String>,
    candidates: &[(&'a str, &Extraction)],
) -> (HashSet<String>, HashSet<&'a str>) {
    let mut by_key: HashMap<String, Vec<&'a str>> = HashMap::new();
    for (path, extraction) in candidates {
        for reference in &extraction.references {
            for (key, from_include) in reference_keys(reference) {
                if from_include {
                    by_key.entry(key).or_default().push(path);
                }
            }
        }
    }
    let mut reached = HashSet::new();
    let mut queue: Vec<String> = keys.iter().cloned().collect();
    while let Some(key) = queue.pop() {
        for &path in by_key.get(&key).map_or(&[][..], Vec::as_slice) {
            reached.insert(path);
            if keys.insert(file_stem(path).to_string()) {
                queue.push(file_stem(path).to_string());
            }
        }
    }
    (keys, reached)
}

/// Whether any reference of the file depends on one of `keys`.
pub fn mentions_any(extraction: &Extraction, keys: &HashSet<String>) -> bool {
    extraction.references.iter().any(|reference| {
        reference_keys(reference)
            .iter()
            .any(|(key, _)| keys.contains(key))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn including(header: &str) -> Extraction {
        Extraction {
            references: vec![Reference {
                kind: ReferenceKind::Include { system: false },
                text: header.to_string(),
                alias: None,
                member: None,
                from: None,
                line: 1,
                arguments: None,
                scope: String::new(),
                receiver_type: None,
                via_self: false,
            }],
            ..Extraction::default()
        }
    }

    fn defining(name: &str, arity: u32) -> Extraction {
        Extraction {
            definitions: vec![Definition {
                kind: NodeKind::Function,
                name: name.to_string(),
                qualified_name: name.to_string(),
                line: 1,
                end_line: 1,
                signature: None,
                doc: None,
                parent: None,
                arity: Some(arity),
            }],
            ..Extraction::default()
        }
    }

    #[test]
    fn a_changed_header_reaches_every_file_up_the_include_chain() {
        let middle = including("deep.h");
        let top = including("middle.h");
        let unrelated = including("other.h");
        let candidates = [
            ("middle.h", &middle),
            ("top.cpp", &top),
            ("x.cpp", &unrelated),
        ];
        let removed = including("something_else.h");
        let keys = changed_keys(&[Change {
            path: "deep.h",
            old: Some(&removed),
            new: None,
        }]);
        let (_, reached) = follow_includes(keys, &candidates);
        assert_eq!(reached, HashSet::from(["middle.h", "top.cpp"]));
    }

    #[test]
    fn an_edit_that_keeps_definitions_and_includes_changes_no_key() {
        let before = defining("f", 1);
        let after = defining("f", 1);
        assert!(
            changed_keys(&[Change {
                path: "a.cpp",
                old: Some(&before),
                new: Some(&after)
            }])
            .is_empty()
        );
    }

    #[test]
    fn a_new_arity_or_name_is_a_key() {
        let before = defining("f", 1);
        let after = defining("f", 2);
        let keys = changed_keys(&[Change {
            path: "a.cpp",
            old: Some(&before),
            new: Some(&after),
        }]);
        assert_eq!(keys, HashSet::from(["f".to_string()]));
    }
}
