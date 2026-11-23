//! Each fixture tree is indexed and the whole graph compared with a golden file: an expected
//! edge that goes missing or an unresolved name that appears fails the test.
//! Regenerate with `UPDATE_GOLDEN=1 cargo test -p index --test it golden` and review the diff.

use crate::common::{assert_golden, copy_fixture, dump, index};

#[test]
fn cpp_fixture_matches_golden() {
    let tree = copy_fixture("cpp", "golden-cpp");
    index(tree.path());
    assert_golden("cpp", &dump(tree.path(), false));
}

#[test]
fn rust_fixture_matches_golden() {
    let tree = copy_fixture("rust", "golden-rust");
    index(tree.path());
    assert_golden("rust", &dump(tree.path(), false));
}

#[test]
fn script_fixture_matches_golden() {
    let tree = copy_fixture("script", "golden-script");
    index(tree.path());
    assert_golden("script", &dump(tree.path(), false));
}

#[test]
fn php_fixture_matches_golden() {
    let tree = copy_fixture("php", "golden-php");
    index(tree.path());
    assert_golden("php", &dump(tree.path(), false));
}

#[test]
fn laravel_fixture_matches_golden() {
    let tree = copy_fixture("laravel", "golden-laravel");
    index(tree.path());
    assert_golden("laravel", &dump(tree.path(), false));
}

#[test]
fn links_fixture_matches_golden() {
    let tree = copy_fixture("links", "golden-links");
    index(tree.path());
    assert_golden("links", &dump(tree.path(), false));
}
