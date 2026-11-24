//! A `use` that goes through another crate's `pub use` must link to the file that defines the
//! item, not stay a dangling import: those links are what the map draws between groups.

use crate::common::{TestDir, dump, index};

#[test]
fn an_import_through_a_reexport_links_to_the_defining_file() {
    let tree = TestDir::new("rust-reexport");
    tree.write("Cargo.toml", "[workspace]\nmembers = [\"crates/lib\", \"crates/app\"]\n");
    tree.write(
        "crates/lib/Cargo.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n",
    );
    tree.write("crates/lib/src/lib.rs", "mod model;\npub use model::Child;\n");
    tree.write("crates/lib/src/model.rs", "pub struct Child;\n");
    tree.write(
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    tree.write("crates/app/src/lib.rs", "use lib::Child;\npub fn f(_: Child) {}\n");
    index(tree.path());

    let edges: Vec<String> = dump(tree.path(), false)
        .lines()
        .filter(|line| line.contains("imports") && line.contains("crates/app/src/lib.rs"))
        .map(str::to_string)
        .collect();

    assert!(
        edges.iter().any(|edge| edge.contains("crates/lib/src/model.rs")),
        "{edges:?}"
    );
    assert!(!edges.iter().any(|edge| edge.contains("does not resolve")), "{edges:?}");
}
