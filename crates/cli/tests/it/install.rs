use serde_json::{Value, json};

use crate::common::{TestDir, ok};

fn names(project: &TestDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(project.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn registers_the_server_and_snippet_in_the_project_only() {
    let project = TestDir::new("install");
    project.write(
        ".mcp.json",
        &json!({"mcpServers": {"other": {"command": "x"}}}).to_string(),
    );
    project.write("CLAUDE.md", "# Mine\n");

    ok(project.path(), &["install"]);
    let mcp: Value = serde_json::from_str(&project.read(".mcp.json")).unwrap();
    assert_eq!(mcp["mcpServers"]["other"]["command"], "x");
    assert_eq!(mcp["mcpServers"]["cartograph"]["args"][0], "mcp");
    let instructions = project.read("CLAUDE.md");
    assert!(instructions.starts_with("# Mine\n") && instructions.contains("`overview`"));

    let before = (project.read(".mcp.json"), project.read("CLAUDE.md"));
    ok(project.path(), &["install"]);
    assert_eq!(
        before,
        (project.read(".mcp.json"), project.read("CLAUDE.md"))
    );
    assert_eq!(names(&project), [".mcp.json", "CLAUDE.md"]);
}

#[test]
fn writes_only_where_the_flags_point_and_the_hook_is_opt_in() {
    let project = TestDir::new("install-flags");
    ok(
        project.path(),
        &[
            "install",
            "--mcp-config",
            "cfg/mcp.json",
            "--instructions",
            "docs/AGENTS.md",
        ],
    );
    assert_eq!(names(&project), ["cfg", "docs"]);
    assert!(!project.read("cfg/mcp.json").contains("PreToolUse"));

    ok(
        project.path(),
        &[
            "install",
            "--mcp-config",
            "cfg/mcp.json",
            "--instructions",
            "docs/AGENTS.md",
            "--strict-hook-settings",
            ".claude/settings.json",
        ],
    );
    let settings: Value = serde_json::from_str(&project.read(".claude/settings.json")).unwrap();
    let entry = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(entry["matcher"], "Read|Grep");
    assert!(
        entry["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("cartograph hook --root ")
    );
}

#[test]
fn a_malformed_config_is_refused_not_overwritten() {
    let project = TestDir::new("install-bad");
    project.write(".mcp.json", "{ not json");
    let output = crate::common::run(project.path(), &["install"]);
    assert!(!output.status.success());
    assert_eq!(project.read(".mcp.json"), "{ not json");
}

#[test]
fn registers_with_codex_only_when_asked_and_keeps_its_settings() {
    let project = TestDir::new("install-codex");
    project.write("codex/config.toml", "model = \"x\"\n");
    ok(
        project.path(),
        &["install", "--codex-config", "codex/config.toml"],
    );
    let config = project.read("codex/config.toml");
    assert!(config.starts_with("model = \"x\"\n"));
    assert!(config.contains("[mcp_servers.cartograph]") && config.contains("\"mcp\""));
    ok(
        project.path(),
        &["install", "--codex-config", "codex/config.toml"],
    );
    assert_eq!(project.read("codex/config.toml"), config);
}
