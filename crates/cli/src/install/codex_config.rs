use std::path::Path;

use serde_json::json;

use super::section::with_marked_section;
use crate::Result;

const BEGIN: &str = "# cartograph:begin";
const END: &str = "# cartograph:end";

/// Codex's `config.toml` text with the cartograph MCP server registered; the rest of the file is
/// kept byte for byte, and the result must still parse as TOML.
pub fn with_server(current: &str, project: &Path) -> Result<String> {
    // A JSON string is also a valid TOML basic string.
    let root = json!(project.to_string_lossy()).to_string();
    let body = format!(
        "[mcp_servers.cartograph]\ncommand = \"cartograph\"\nargs = [\"mcp\", \"--root\", {root}]"
    );
    let updated = with_marked_section(current, BEGIN, END, &body);
    toml::from_str::<toml::Table>(&updated)
        .map_err(|error| format!("the Codex config would not be valid TOML: {error}"))?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_once_and_keeps_the_rest() {
        let once = with_server("model = \"x\"\n", Path::new("/p")).unwrap();
        assert!(once.starts_with("model = \"x\"\n\n# cartograph:begin"));
        let table: toml::Table = toml::from_str(&once).unwrap();
        assert_eq!(
            table["mcp_servers"]["cartograph"]["args"][2].as_str(),
            Some("/p")
        );
        assert_eq!(with_server(&once, Path::new("/p")).unwrap(), once);
    }

    #[test]
    fn refuses_a_config_that_already_defines_the_server_elsewhere() {
        let existing = "[mcp_servers.cartograph]\ncommand = \"old\"\n";
        assert!(with_server(existing, Path::new("/p")).is_err());
    }
}
