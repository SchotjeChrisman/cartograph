use std::os::unix::fs::PermissionsExt;

use crate::common::{TestDir, command, indexed_project, ok};

/// A `claude` that answers every prompt with a fixed valid description.
fn fake_claude() -> TestDir {
    let bin = TestDir::executable("fake-claude");
    bin.write(
        "claude",
        "#!/bin/sh\ncat > /dev/null\necho '{\"name\":\"Fake Name\",\"paragraph\":\"A part written by the fake model.\",\"tags\":[\"fake\"]}'\n",
    );
    let path = bin.path().join("claude");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    bin
}

fn describe_with(bin: &TestDir, dir: &std::path::Path, args: &[&str]) -> String {
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let output = command(dir).env("PATH", path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn describe_writes_descriptions_once_and_overview_shows_them() {
    let project = indexed_project("describe");
    let dir = project.path();
    ok(dir, &["cluster"]);
    let bin = fake_claude();

    let first = describe_with(&bin, dir, &["describe", "--depth", "1"]);
    assert!(first.contains(": described"), "{first}");
    let second = describe_with(&bin, dir, &["describe", "--depth", "1"]);
    assert!(
        second.contains("already described") && !second.contains(": described"),
        "{second}"
    );

    let overview = ok(dir, &["overview"]);
    assert!(
        overview.contains("Fake Name") || overview.contains("fake model"),
        "{overview}"
    );
}

#[test]
fn a_failing_backend_surfaces_its_error() {
    let project = indexed_project("describe-fail");
    ok(project.path(), &["cluster"]);
    let output = command(project.path())
        .args(["describe", "--backend", "ollama:no-such-model"])
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ollama"));
}
