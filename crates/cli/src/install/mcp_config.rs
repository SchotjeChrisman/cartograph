use std::path::Path;

use serde_json::{Value, json};

use crate::Result;

const SERVER_NAME: &str = "cartograph";

/// `config` with the cartograph server registered; other servers are kept.
pub fn with_server(mut config: Value, project: &Path) -> Result<Value> {
    let servers = config
        .as_object_mut()
        .expect("caller passes a JSON object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("`mcpServers` in the MCP config is not an object")?;
    servers.insert(
        SERVER_NAME.to_string(),
        json!({
            "command": "cartograph",
            "args": ["mcp", "--root", project.to_string_lossy()],
        }),
    );
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_other_servers_and_replaces_its_own() {
        let existing =
            json!({"mcpServers": {"other": {"command": "x"}, "cartograph": {"command": "old"}}});
        let updated = with_server(existing, Path::new("/p")).unwrap();
        assert_eq!(updated["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            updated["mcpServers"]["cartograph"]["args"],
            json!(["mcp", "--root", "/p"])
        );
    }

    #[test]
    fn refuses_a_malformed_servers_entry() {
        assert!(with_server(json!({"mcpServers": []}), Path::new("/p")).is_err());
    }
}
