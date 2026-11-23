use std::path::Path;

use serde_json::{Value, json};

use crate::Result;

/// Tools the strict hook guards.
const GUARDED_TOOLS: &str = "Read|Grep";
const HOOK_COMMAND_PREFIX: &str = "cartograph hook";

/// `settings` with the strict PreToolUse hook registered once; other hooks are kept.
pub fn with_strict_hook(mut settings: Value, project: &Path) -> Result<Value> {
    let hooks = settings
        .as_object_mut()
        .expect("caller passes a JSON object")
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("`hooks` in the settings file is not an object")?;
    let pre_tool_use = hooks
        .entry("PreToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("`hooks.PreToolUse` in the settings file is not an array")?;
    pre_tool_use.retain(|entry| !is_cartograph_entry(entry));
    pre_tool_use.push(json!({
        "matcher": GUARDED_TOOLS,
        "hooks": [{
            "type": "command",
            "command": format!("{HOOK_COMMAND_PREFIX} --root {}", project.to_string_lossy()),
        }],
    }));
    Ok(settings)
}

fn is_cartograph_entry(entry: &Value) -> bool {
    entry["hooks"].as_array().is_some_and(|hooks| {
        hooks.iter().any(|hook| {
            hook["command"]
                .as_str()
                .is_some_and(|command| command.starts_with(HOOK_COMMAND_PREFIX))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_once_and_keeps_foreign_hooks() {
        let foreign = json!({"matcher": "Bash", "hooks": [{"type": "command", "command": "lint"}]});
        let existing = json!({"hooks": {"PreToolUse": [foreign.clone()]}});
        let once = with_strict_hook(existing, Path::new("/p")).unwrap();
        let twice = with_strict_hook(once.clone(), Path::new("/p")).unwrap();
        assert_eq!(once, twice);
        let entries = twice["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], foreign);
        assert_eq!(
            entries[1]["hooks"][0]["command"],
            "cartograph hook --root /p"
        );
    }
}
