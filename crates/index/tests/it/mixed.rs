//! Repositories mixing languages: a name resolves only inside its own language family, and a
//! call nothing defines is kept as an unresolved edge so coverage can count it.

use crate::common::{TestDir, dump, index};

fn call_edges(tree: &TestDir) -> Vec<String> {
    dump(tree.path(), false)
        .lines()
        .filter(|line| line.contains("calls"))
        .map(str::to_string)
        .collect()
}

#[test]
fn a_php_call_does_not_resolve_to_a_javascript_function() {
    let tree = TestDir::new("mixed-php-js");
    tree.write("a.php", "<?php\nfunction g() { helper(); }\n");
    tree.write("b.js", "function helper() {}\n");
    index(tree.path());
    let edges = call_edges(&tree);
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(edges[0].contains("unresolved"), "{edges:?}");
}

#[test]
fn a_cpp_call_does_not_resolve_to_a_php_function() {
    let tree = TestDir::new("mixed-cpp-php");
    tree.write("a.cpp", "void run() { helper(1); }\n");
    tree.write("b.php", "<?php\nfunction helper($a) {}\n");
    index(tree.path());
    let edges = call_edges(&tree);
    assert!(
        edges.iter().all(|edge| edge.contains("unresolved")),
        "{edges:?}"
    );
}

#[test]
fn a_local_php_function_stays_unambiguous_beside_a_javascript_namesake() {
    let tree = TestDir::new("mixed-php-local");
    tree.write("b.php", "<?php\nfunction f() {}\nfunction g() { f(); }\n");
    tree.write("a.js", "function f() {}\n");
    index(tree.path());
    let edges = call_edges(&tree);
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(
        edges[0].contains("b.php") && !edges[0].contains("ambiguous"),
        "{edges:?}"
    );
}

#[test]
fn javascript_and_typescript_still_resolve_into_each_other() {
    let tree = TestDir::new("mixed-js-ts");
    tree.write(
        "a.ts",
        "import { helper } from './b.js'\nexport function g() { helper() }\n",
    );
    tree.write("b.js", "export function helper() {}\n");
    index(tree.path());
    let edges = call_edges(&tree);
    assert!(
        edges
            .iter()
            .any(|edge| edge.contains("b.js") && !edge.contains("unresolved")),
        "{edges:?}"
    );
}

#[test]
fn calls_to_missing_functions_are_kept_as_unresolved_edges() {
    let tree = TestDir::new("unresolved-free-calls");
    tree.write("a.js", "function g() { zzz(); }\n");
    tree.write(
        "b.php",
        "<?php\nfunction f() {}\nfunction g() { f(); zzz(); }\n",
    );
    tree.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    tree.write("src/lib.rs", "pub fn g() { nothere(); Some(1); }\n");
    index(tree.path());
    let edges = call_edges(&tree);
    let unresolved: Vec<_> = edges
        .iter()
        .filter(|edge| edge.contains("unresolved"))
        .collect();
    for file in ["a.js", "b.php", "src/lib.rs"] {
        assert!(
            unresolved
                .iter()
                .any(|edge| edge.contains(file) && edge.contains("no definition found")),
            "{file}: {edges:?}"
        );
    }
    assert!(!edges.iter().any(|edge| edge.contains("Some")), "{edges:?}");
}
