//! `cartograph mcp`: the six query tools over MCP, a JSON-RPC conversation on stdio. Each call
//! runs the same path as the CLI (open the graph, staleness check, budgeted answer), so a
//! long-lived server never serves a stale snapshot.

mod rpc;
mod tools;

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::{Result, ask};
use rpc::{INVALID_PARAMS, METHOD_NOT_FOUND, Message};
use tools::Arguments;

/// Newest first; the first one the client also speaks is the one used.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const SERVER_NAME: &str = "cartograph";
const INSTRUCTIONS: &str = "Ask these tools before reading or grepping: `overview` for the \
shape of the code, `find` to resolve a name to an @id, then `explore`, `path` or `impact` on \
it. `coverage` lists files the graph did not parse; read those directly. Every answer is cut \
at `budget` tokens and ends with the exact call that shows more.";

/// Answers requests from `input` until it closes.
pub fn serve(root: &Path, input: impl BufRead, mut output: impl Write) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(root, &line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn handle_line(root: &Path, line: &str) -> Option<Value> {
    let message = match rpc::parse(line) {
        Ok(message) => message,
        Err(reply) => return Some(reply),
    };
    let id = message.id.clone()?;
    Some(match dispatch(root, &message) {
        Ok(result) => rpc::result_reply(id, result),
        Err((code, text)) => rpc::error_reply(id, code, &text),
    })
}

fn dispatch(root: &Path, message: &Message) -> std::result::Result<Value, (i64, String)> {
    match message.method.as_str() {
        "initialize" => Ok(initialize(&message.params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => call_tool(root, &message.params),
        other => Err((
            METHOD_NOT_FOUND,
            format!("method `{other}` is not supported"),
        )),
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params["protocolVersion"].as_str().unwrap_or_default();
    let version = PROTOCOL_VERSIONS
        .iter()
        .find(|supported| **supported == requested)
        .unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn call_tool(root: &Path, params: &Value) -> std::result::Result<Value, (i64, String)> {
    let name = params["name"]
        .as_str()
        .ok_or((INVALID_PARAMS, "`name` is required".to_string()))?;
    let no_arguments = json!({});
    let raw_arguments = params
        .get("arguments")
        .filter(|value| !value.is_null())
        .unwrap_or(&no_arguments);
    let arguments = match Arguments::new(raw_arguments) {
        Ok(arguments) => arguments,
        Err(error) => return Ok(tool_failure(&error.0)),
    };
    let Some(built) = tools::request(name, &arguments) else {
        return Err((INVALID_PARAMS, format!("unknown tool `{name}`")));
    };
    let outcome = built.map_err(|error| error.0).and_then(|request| {
        let as_json = arguments.wants_json().map_err(|error| error.0)?;
        let output = ask::answer(root, &request).map_err(|error| error.to_string())?;
        tool_success(&output, as_json).map_err(|error| error.to_string())
    });
    Ok(outcome.unwrap_or_else(|message| tool_failure(&message)))
}

fn tool_success(output: &query::Output, as_json: bool) -> Result<Value> {
    let text = ask::render(output, as_json)?;
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if as_json {
        result["structuredContent"] = serde_json::to_value(output)?;
    }
    Ok(result)
}

/// A failed call is a result the model can read and react to, not a protocol error.
fn tool_failure(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(lines: &[&str]) -> Vec<Value> {
        let input = lines.join("\n");
        let mut output = Vec::new();
        serve(Path::new("/nonexistent"), input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn handshake_lists_six_tools_and_notifications_get_no_reply() {
        let replies = exchange(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ]);
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn protocol_problems_are_errors_and_tool_problems_are_results() {
        let replies = exchange(&[
            "not json",
            r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"coverage"}}"#,
        ]);
        assert_eq!(replies[0]["error"]["code"], rpc::PARSE_ERROR);
        assert_eq!(replies[1]["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(replies[2]["error"]["code"], INVALID_PARAMS);
        assert_eq!(replies[3]["result"]["isError"], true);
        assert_eq!(replies[4]["result"]["isError"], true);
        let text = replies[4]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("cartograph index"), "{text}");
    }
}
