use std::fs::File;
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use serde_json::json;

use crate::common::{TestDir, command, indexed_project, ok};

fn hook(project: &TestDir, event: serde_json::Value) -> String {
    let mut child = command(project.path())
        .arg("hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn read_event(project: &TestDir, relative: &str) -> serde_json::Value {
    let file_path = project.path().join(relative);
    json!({"tool_name": "Read", "tool_input": {"file_path": file_path}, "cwd": project.path()})
}

fn grep_event(project: &TestDir, relative: &str) -> serde_json::Value {
    let path = project.path().join(relative);
    json!({"tool_name": "Grep", "tool_input": {"pattern": "x", "path": path}, "cwd": project.path()})
}

#[test]
fn denies_read_and_grep_until_a_query_then_lets_them_through() {
    let project = indexed_project("hook");
    let denied = hook(&project, read_event(&project, "crates/app/src/main.rs"));
    let decision: serde_json::Value = serde_json::from_str(&denied).unwrap();
    assert_eq!(decision["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        decision["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("cartograph")
    );
    assert!(hook(&project, grep_event(&project, "crates")).contains("deny"));

    ok(project.path(), &["find", "leaf"]);
    assert_eq!(
        hook(&project, read_event(&project, "crates/app/src/main.rs")),
        ""
    );
    assert_eq!(hook(&project, grep_event(&project, "crates")), "");
}

#[test]
fn an_old_orientation_no_longer_counts() {
    let project = indexed_project("hook-expiry");
    ok(project.path(), &["find", "leaf"]);
    let marker = project.path().join(".cartograph/oriented");
    let long_ago = SystemTime::now() - Duration::from_secs(3 * 3600);
    File::options()
        .write(true)
        .open(marker)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    assert!(hook(&project, read_event(&project, "crates/app/src/main.rs")).contains("deny"));
}

#[test]
fn unparsed_files_stay_readable_and_greppable() {
    let project = indexed_project("hook-unparsed");
    assert_eq!(hook(&project, read_event(&project, "scripts/tool.py")), "");
    assert_eq!(hook(&project, read_event(&project, "README.md")), "");
    assert_eq!(hook(&project, grep_event(&project, "scripts")), "");
}

#[test]
fn a_whole_tree_grep_is_denied_while_parsed_code_is_in_it() {
    let project = indexed_project("hook-whole-tree");
    let whole_tree =
        json!({"tool_name": "Grep", "tool_input": {"pattern": "x"}, "cwd": project.path()});
    assert!(hook(&project, whole_tree).contains("deny"));
}

#[test]
fn a_directory_without_graph_files_stays_greppable() {
    let project = indexed_project("hook-docs-only");
    project.write("docs/a.md", "# notes\n");
    ok(project.path(), &["index"]);
    assert_eq!(hook(&project, read_event(&project, "docs/a.md")), "");
    assert_eq!(hook(&project, grep_event(&project, "docs")), "");
}

#[test]
fn without_a_graph_the_hook_never_blocks() {
    let project = TestDir::new("hook-no-graph");
    project.write("a.rs", "fn a() {}\n");
    assert_eq!(hook(&project, read_event(&project, "a.rs")), "");
}
