use std::io::Write;
use std::process::Stdio;

use serde_json::{Value, json};

use crate::common::{command, indexed_project};

fn conversation(dir: &std::path::Path, messages: &[Value]) -> Vec<Value> {
    let mut child = command(dir)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn call(id: u32, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

#[test]
fn serves_the_six_tools_over_stdio_with_budgets() {
    let project = indexed_project("mcp");
    let replies = conversation(
        project.path(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(3, "find", json!({"text": "leaf"})),
            call(4, "find", json!({"text": "a", "budget": 40})),
            call(
                5,
                "explore",
                json!({"target": "helper", "relation": "callers"}),
            ),
            call(6, "path", json!({"from": "leaf", "to": "helper"})),
            call(7, "impact", json!({"target": "helper"})),
            call(8, "coverage", json!({})),
            call(9, "overview", json!({})),
            call(10, "find", json!({"text": "leaf", "json": true})),
            call(
                11,
                "explore",
                json!({"target": "x", "relation": "sideways"}),
            ),
        ],
    );
    assert_eq!(replies.len(), 11, "the notification gets no reply");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    let tools: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        ["overview", "find", "explore", "path", "impact", "coverage"]
    );

    let text = |index: usize| {
        replies[index]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(text(2).contains("util.rs"));
    assert!(text(3).contains("Next: cartograph find"), "{}", text(3));
    assert!(text(4).contains("leaf"));
    assert!(text(5).contains("helper"));
    assert!(text(6).contains("leaf"));
    assert!(text(7).contains("scripts/tool.py"));
    assert_eq!(
        replies[9]["result"]["structuredContent"]["report"]["tool"],
        "find"
    );
    assert_eq!(replies[10]["result"]["isError"], true);
    assert!(text(10).contains("relation"));
}

#[test]
fn a_call_after_an_edit_sees_the_new_code() {
    let project = indexed_project("mcp-fresh");
    crate::common::add_function(
        &project,
        "crates/core_lib/src/util.rs",
        "added_during_session",
    );
    let replies = conversation(
        project.path(),
        &[call(1, "find", json!({"text": "added_during_session"}))],
    );
    let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("re-indexed") && text.contains("util.rs"),
        "{text}"
    );
}
