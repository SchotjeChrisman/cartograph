use serde_json::Value;

use crate::common::{TestDir, indexed_project, ok, run};

#[test]
fn index_reports_and_a_second_run_changes_nothing() {
    let project = crate::common::rust_project("index-twice");
    let first = ok(project.path(), &["index"]);
    assert!(first.starts_with("indexed "), "{first}");
    assert!(first.contains("parsed"), "{first}");
    let second = ok(project.path(), &["index", "--json"]);
    let report: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(report["parsed"], 0);
    assert_eq!(report["deleted"], 0);
}

#[test]
fn coverage_lists_the_unparsed_python_file() {
    let project = indexed_project("coverage");
    let text = ok(project.path(), &["coverage"]);
    assert!(text.contains("scripts/tool.py"), "{text}");
}

#[test]
fn find_callers_callees_path_and_impact_answer_from_the_graph() {
    let project = indexed_project("queries");
    let dir = project.path();
    let found = ok(dir, &["find", "leaf"]);
    assert!(found.contains("util.rs"), "{found}");

    let callers = ok(dir, &["callers", "helper"]);
    assert!(callers.contains("leaf"), "{callers}");
    let callees = ok(dir, &["callees", "leaf"]);
    assert!(callees.contains("helper"), "{callees}");
    let explored = ok(dir, &["explore", "helper", "--callers"]);
    assert_eq!(explored, callers);

    let path = ok(dir, &["path", "leaf", "helper"]);
    assert!(path.contains("helper"), "{path}");
    let impact = ok(dir, &["impact", "helper", "--depth", "3"]);
    assert!(impact.contains("leaf"), "{impact}");
}

#[test]
fn json_output_is_the_structured_report() {
    let project = indexed_project("json");
    let text = ok(project.path(), &["--json", "find", "leaf"]);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["report"]["tool"], "find");
    assert!(!value["report"]["candidates"].as_array().unwrap().is_empty());
}

#[test]
fn a_small_budget_cuts_the_answer_and_the_footer_command_runs_as_printed() {
    let project = indexed_project("budget");
    let dir = project.path();
    let cut = ok(dir, &["find", "a", "--budget", "40"]);
    let footer = cut
        .lines()
        .find(|line| line.contains("Next: "))
        .unwrap_or_else(|| panic!("no footer in {cut}"));
    let command = footer.split("Next: ").nth(1).unwrap();
    let args: Vec<&str> = command.split_whitespace().skip(1).collect();
    assert_eq!(&args[..2], ["find", "a"]);
    let next = ok(dir, &args);
    assert_ne!(next, cut);
}

#[test]
fn cluster_makes_overview_show_components() {
    let project = indexed_project("cluster");
    let clustered = ok(project.path(), &["cluster"]);
    assert!(clustered.starts_with("clustered "), "{clustered}");
    let overview = ok(project.path(), &["overview"]);
    assert!(overview.lines().count() > 1, "{overview}");
}

#[test]
fn queries_find_the_graph_from_a_subdirectory() {
    let project = indexed_project("subdir");
    let sub = project.path().join("crates/app");
    let found = ok(&sub, &["find", "leaf"]);
    assert!(found.contains("util.rs"), "{found}");
}

#[test]
fn a_missing_graph_fails_loudly_with_the_fix() {
    let empty = TestDir::new("no-graph");
    let output = run(empty.path(), &["find", "x"]);
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("cartograph index"), "{message}");
}
