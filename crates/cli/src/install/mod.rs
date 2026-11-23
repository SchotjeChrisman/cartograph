//! `cartograph install`: writes the MCP registration and the agent instructions into the files
//! it is given, or into the project's own `.mcp.json` and `CLAUDE.md`. It never touches a file
//! outside what the flags and the project name.

mod codex_config;
mod hook_settings;
mod instructions;
mod mcp_config;
mod section;

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Result;
use crate::args::InstallArgs;

const DEFAULT_MCP_CONFIG: &str = ".mcp.json";
const DEFAULT_INSTRUCTIONS: &str = "CLAUDE.md";

pub fn run(project: &Path, args: &InstallArgs) -> Result<String> {
    let mcp_path = args
        .mcp_config
        .clone()
        .unwrap_or_else(|| project.join(DEFAULT_MCP_CONFIG));
    let instructions_path = args
        .instructions
        .clone()
        .unwrap_or_else(|| project.join(DEFAULT_INSTRUCTIONS));

    let mcp = mcp_config::with_server(read_json_object(&mcp_path)?, project)?;
    write_json(&mcp_path, &mcp)?;
    let current = read_text(&instructions_path)?;
    ensure_parent(&instructions_path)?;
    std::fs::write(&instructions_path, instructions::with_snippet(&current))?;
    let mut written = vec![mcp_path, instructions_path];

    if let Some(settings_path) = &args.strict_hook_settings {
        let settings = hook_settings::with_strict_hook(read_json_object(settings_path)?, project)?;
        write_json(settings_path, &settings)?;
        written.push(settings_path.clone());
    }
    if let Some(codex_path) = &args.codex_config {
        let updated = codex_config::with_server(&read_text(codex_path)?, project)?;
        ensure_parent(codex_path)?;
        std::fs::write(codex_path, updated)?;
        written.push(codex_path.clone());
    }
    let lines: Vec<String> = written
        .iter()
        .map(|path| format!("updated {}", path.display()))
        .collect();
    Ok(lines.join("\n"))
}

/// The file's JSON object, or an empty one when the file does not exist yet.
fn read_json_object(path: &Path) -> Result<Value> {
    let text = read_text(path)?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: not valid JSON: {error}", path.display()))?;
    if !value.is_object() {
        return Err(format!(
            "{}: expected a JSON object at the top level",
            path.display()
        )
        .into());
    }
    Ok(value)
}

fn read_text(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("{}: {error}", path.display()).into()),
    }
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    ensure_parent(path)?;
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(parent) if parent != PathBuf::new() => Ok(std::fs::create_dir_all(parent)?),
        _ => Ok(()),
    }
}
