//! The incremental index must equal a clean full index after every kind of change.

use index::{IndexOptions, index_directory};

use crate::common::{TestDir, assert_incremental_equals_full, copy_fixture, dump, index};

#[test]
fn a_second_run_without_changes_does_nothing() {
    let tree = copy_fixture("cpp", "inc-noop");
    let first = index(tree.path());
    let before = dump(tree.path(), true);
    let second = index(tree.path());
    assert_eq!(second.parsed, 0);
    assert_eq!(second.re_resolved, 0);
    assert_eq!(second.unchanged, first.files);
    assert_eq!(second.loaded.files, 0);
    assert_eq!(dump(tree.path(), true), before);
}

#[test]
fn cpp_edits_additions_and_deletions() {
    let tree = copy_fixture("cpp", "inc-cpp");
    index(tree.path());

    // Removing the second helper() changes what the untouched lonely.cpp resolves to.
    tree.write("src/other.cpp", "int other_entry() { return 0; }\n");
    let report = assert_incremental_equals_full(&tree, "inc-cpp-edit");
    assert_eq!(report.parsed, 1);
    assert!(
        report.re_resolved >= 1,
        "lonely.cpp must be resolved again: {report:?}"
    );

    // A header that was missing appears, so main.cpp's unresolved include resolves.
    tree.write("src/no_such_header.h", "#pragma once\n");
    assert_incremental_equals_full(&tree, "inc-cpp-add-header");

    // A third dup() keeps the call ambiguous but changes the candidate count.
    tree.write("src/dup_c.cpp", "int dup(int x) { return x + 2; }\n");
    assert_incremental_equals_full(&tree, "inc-cpp-add-dup");

    // The implementation of util goes away: callers lose their targets.
    tree.remove("src/util/strings.cpp");
    let report = assert_incremental_equals_full(&tree, "inc-cpp-delete");
    assert_eq!(report.deleted, 1);

    // A file starts matching the vendored pattern through the config.
    tree.write(".cartograph/config.toml", "vendored = [\"src/app/\"]\n");
    assert_incremental_equals_full(&tree, "inc-cpp-config");

    // A file leaves the walk by being gitignored.
    tree.write(".gitignore", "build/\nsrc/dup_a.cpp\n");
    assert_incremental_equals_full(&tree, "inc-cpp-gitignore");
}

#[test]
fn a_changed_header_deep_in_the_include_chain_changes_visibility_up_the_chain() {
    let tree = TestDir::new("inc-chain");
    tree.write(
        "a.cpp",
        "#include \"mid.h\"\nint caller() { return target(1); }\n",
    );
    tree.write("mid.h", "#include \"deep.h\"\n");
    tree.write("deep.h", "// nothing yet\n");
    tree.write("deeper.h", "int target(int x);\n");
    tree.write("deeper.cpp", "int target(int x) { return x; }\n");
    tree.write("other.cpp", "int target(int x) { return 2; }\n");
    index(tree.path());
    assert!(
        dump(tree.path(), false).contains("-> unresolved target"),
        "ambiguous before the include exists"
    );

    tree.write("deep.h", "#include \"deeper.h\"\n");
    assert_incremental_equals_full(&tree, "inc-chain-link");
    let after = dump(tree.path(), false);
    assert!(
        after.contains("function caller @a.cpp -calls/inferred-> function target @deeper.cpp"),
        "visibility through a.cpp -> mid.h -> deep.h -> deeper.h -> deeper.cpp resolves the call:\n{after}"
    );
}

#[test]
fn rust_edits_additions_and_deletions() {
    let tree = copy_fixture("rust", "inc-rust");
    index(tree.path());

    let util = tree
        .read("crates/core_lib/src/util.rs")
        .replace("pub fn helper()", "pub fn helper_renamed()");
    tree.write("crates/core_lib/src/util.rs", &util);
    assert_incremental_equals_full(&tree, "inc-rust-rename");

    tree.write("crates/core_lib/src/gone.rs", "pub fn back() {}\n");
    assert_incremental_equals_full(&tree, "inc-rust-add-module");

    tree.remove("crates/app/src/local.rs");
    assert_incremental_equals_full(&tree, "inc-rust-delete-module");

    // The crate name feeds every qualified name, so every file is extracted again.
    let manifest = tree
        .read("crates/core_lib/Cargo.toml")
        .replace("core-lib", "core-renamed");
    tree.write("crates/core_lib/Cargo.toml", &manifest);
    let report = assert_incremental_equals_full(&tree, "inc-rust-crate-rename");
    assert_eq!(report.parsed, report.files);
}

#[test]
fn script_edits_move_resolution_in_unchanged_importers() {
    let tree = copy_fixture("script", "inc-script");
    index(tree.path());

    // The missing import appears, so main.ts resolves it without being touched.
    tree.write("src/not_there.ts", "export function missing() {}\n");
    assert_incremental_equals_full(&tree, "inc-script-add-module");
    assert!(!dump(tree.path(), false).contains("unresolved src/not_there"));

    // A third `render` keeps the call ambiguous but changes the candidate count.
    tree.write(
        "src/shapes/triangle.ts",
        "export class Triangle { render() { return 't'; } }\n",
    );
    assert_incremental_equals_full(&tree, "inc-script-add-render");

    // Re-exporting module goes away: the import of ./shapes loses its target.
    tree.remove("src/shapes/index.ts");
    let report = assert_incremental_equals_full(&tree, "inc-script-delete-index");
    assert_eq!(report.deleted, 1);
}

#[test]
fn php_edits_move_resolution_in_unchanged_callers() {
    let tree = copy_fixture("php", "inc-php");
    index(tree.path());

    // A second `slug` makes the `use function` call ambiguous.
    tree.write("src/Util/other_helpers.php", "<?php\n\nnamespace App\\Util;\n\nfunction slug(string $text): string\n{\n    return $text;\n}\n");
    assert_incremental_equals_full(&tree, "inc-php-second-slug");

    // The base class loses `prefix`: Greeter's call to it no longer resolves.
    let greeter = tree
        .read("src/Greeter.php")
        .replace("function prefix", "function prefixed");
    tree.write("src/Greeter.php", &greeter);
    assert_incremental_equals_full(&tree, "inc-php-rename-method");
}

#[test]
fn laravel_edits_move_routes_views_and_hooks() {
    let tree = copy_fixture("laravel", "inc-laravel");
    index(tree.path());

    // The missing view file appears, so the untouched Blade include resolves.
    tree.write(
        "resources/views/partials/missing.blade.php",
        "<p>here</p>\n",
    );
    assert_incremental_equals_full(&tree, "inc-laravel-add-view");
    assert!(!dump(tree.path(), false).contains("unresolved view partials.missing"));

    // The controller action the route names appears.
    let controller = tree
        .read("app/Http/Controllers/UserController.php")
        .replace(
            "public function show",
            "public function audit() {}\n\n    public function show",
        );
    tree.write("app/Http/Controllers/UserController.php", &controller);
    assert_incremental_equals_full(&tree, "inc-laravel-add-action");

    // A listener loses its handler; the event it registered for stays in the graph.
    tree.write(
        "app/Listeners/SendWelcomeEmail.php",
        "<?php\n\nnamespace App\\Listeners;\n\nclass SendWelcomeEmail\n{\n}\n",
    );
    assert_incremental_equals_full(&tree, "inc-laravel-listener");

    // Dropping Laravel from composer.json turns the plugin off for every file.
    tree.write("composer.json", "{ \"require\": { \"php\": \"^8.2\" } }\n");
    assert_incremental_equals_full(&tree, "inc-laravel-off");
    assert!(!dump(tree.path(), false).contains("handles_route"));
}

#[test]
fn links_follow_the_graph_they_point_into() {
    let tree = copy_fixture("links", "inc-links");
    index(tree.path());

    // `send` stops being ambiguous once one of the two is gone.
    tree.remove("app/Mailer2.php");
    assert_incremental_equals_full(&tree, "inc-links-remove-candidate");

    // The declared target appears.
    tree.write("app/Nowhere.php", "<?php\n\nnamespace App;\n\nclass Nowhere\n{\n    public function run(): void\n    {\n    }\n}\n");
    assert_incremental_equals_full(&tree, "inc-links-target-appears");
    assert!(!dump(tree.path(), false).contains("unresolved App\\Nowhere::run"));

    // A broken links file is reported and its edges go away.
    tree.write("links.yaml", "links:\n  - to: x\n");
    assert_incremental_equals_full(&tree, "inc-links-broken");
    assert!(dump(tree.path(), false).contains("links.yaml repo= lang=links status=parse_failed"));

    // The links file is deleted.
    tree.remove("links.yaml");
    assert_incremental_equals_full(&tree, "inc-links-deleted");
}

mod modification_time_shortcut {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::*;

    const LONG_AGO: Duration = Duration::from_secs(3600);

    fn set_modified(tree: &TestDir, relative: &str, time: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(tree.path().join(relative))
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    fn modified(tree: &TestDir, relative: &str) -> SystemTime {
        std::fs::metadata(tree.path().join(relative))
            .unwrap()
            .modified()
            .unwrap()
    }

    fn recorded_mtime_ns(tree: &TestDir, relative: &str) -> Option<i64> {
        Connection::open(tree.path().join(".cartograph/graph.db"))
            .unwrap()
            .query_row(
                "SELECT mtime_ns FROM files WHERE path = ?1",
                [relative],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn nanos(time: SystemTime) -> i64 {
        i64::try_from(time.duration_since(UNIX_EPOCH).unwrap().as_nanos()).unwrap()
    }

    fn tree_with_an_old_file(name: &str) -> (TestDir, SystemTime) {
        let tree = TestDir::new(name);
        tree.write("a.c", "int one(void) { return 1; }\n");
        tree.write("b.c", "int two(void) { return 2; }\n");
        let long_ago = SystemTime::now() - LONG_AGO;
        set_modified(&tree, "a.c", long_ago);
        set_modified(&tree, "b.c", long_ago);
        (tree, long_ago)
    }

    #[test]
    fn a_file_with_the_recorded_size_and_time_is_not_read_again() {
        let (tree, long_ago) = tree_with_an_old_file("mtime-skip");
        index(tree.path());

        // Same size, same time: indistinguishable without reading, which is the point.
        tree.write("a.c", "int uno(void) { return 1; }\n");
        set_modified(&tree, "a.c", long_ago);
        assert_eq!(index(tree.path()).parsed, 0);

        set_modified(&tree, "a.c", long_ago + Duration::from_secs(1));
        assert_eq!(index(tree.path()).parsed, 1);
        assert!(dump(tree.path(), false).contains("function uno @a.c"));
    }

    #[test]
    fn a_file_modified_moments_ago_is_always_read() {
        let tree = TestDir::new("mtime-racy");
        tree.write("a.c", "int one(void) { return 1; }\n");
        let moments_ago = modified(&tree, "a.c");
        index(tree.path());
        assert_eq!(recorded_mtime_ns(&tree, "a.c"), None);

        tree.write("a.c", "int uno(void) { return 1; }\n");
        set_modified(&tree, "a.c", moments_ago);
        assert_eq!(index(tree.path()).parsed, 1);
        assert!(dump(tree.path(), false).contains("function uno @a.c"));
    }

    #[test]
    fn a_touched_file_is_read_once_and_then_skipped() {
        let (tree, long_ago) = tree_with_an_old_file("mtime-touch");
        index(tree.path());
        assert_eq!(recorded_mtime_ns(&tree, "a.c"), Some(nanos(long_ago)));

        let touched = long_ago + Duration::from_secs(60);
        set_modified(&tree, "a.c", touched);
        let report = index(tree.path());
        assert_eq!((report.parsed, report.unchanged), (0, 2));
        assert_eq!(recorded_mtime_ns(&tree, "a.c"), Some(nanos(touched)));
    }

    #[test]
    fn a_run_that_fails_midway_makes_the_next_one_parse_everything() {
        let (tree, long_ago) = tree_with_an_old_file("mtime-failed");
        index(tree.path());

        // The extraction cache takes the new content, then the graph refuses it: the cache is
        // ahead of the graph, so nothing in it may be trusted for the old side of a comparison.
        tree.write("a.c", "int uno(void) { return 1; }\n");
        set_modified(&tree, "a.c", long_ago + Duration::from_secs(1));
        let database = Connection::open(tree.path().join(".cartograph/graph.db")).unwrap();
        database
            .execute_batch(
                "CREATE TRIGGER refuse_edges BEFORE INSERT ON edges
                 BEGIN SELECT RAISE(ABORT, 'refused for the test'); END",
            )
            .unwrap();
        let failed = index_directory(tree.path(), &IndexOptions::default());
        database.execute_batch("DROP TRIGGER refuse_edges").unwrap();
        assert!(failed.is_err(), "the refused edges must fail the run");

        assert_eq!(index(tree.path()).parsed, 2);
        assert_incremental_equals_full(&tree, "mtime-failed-run");
    }
}
