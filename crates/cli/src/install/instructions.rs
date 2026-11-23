use super::section::with_marked_section;

const BEGIN: &str = "<!-- cartograph:begin -->";
const END: &str = "<!-- cartograph:end -->";

const SNIPPET: &str = "\
## Code graph (cartograph)

A code graph of this repository is served by the `cartograph` MCP tools (or the `cartograph` CLI). Ask it before reading files or grepping:

- `overview`: components and their most connected symbols. Start here in an unfamiliar area.
- `find`: resolve a name or words to candidates. Each has an `@id` to pass on.
- `explore`: callers, callees or contents of one node.
- `path`: how one node reaches another. `impact`: what breaks if a node changes.
- `coverage`: what the graph did not parse. Read and grep those files directly.

Every answer is cut at a token `budget` and ends with the exact command that shows more.";

/// `current` with the cartograph section added, or replaced in place when it is already there.
pub fn with_snippet(current: &str) -> String {
    with_marked_section(current, BEGIN, END, SNIPPET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_once_and_is_idempotent() {
        let once = with_snippet("# Project\n\nNotes.\n");
        assert!(once.starts_with("# Project\n\nNotes.\n\n<!-- cartograph:begin -->"));
        assert_eq!(with_snippet(&once), once);
    }

    #[test]
    fn replaces_an_old_section_and_keeps_the_text_after_it() {
        let old = format!("before\n\n{BEGIN}\nstale\n{END}\nafter\n");
        let updated = with_snippet(&old);
        assert!(updated.starts_with("before\n\n<!-- cartograph:begin -->\n## Code graph"));
        assert!(updated.ends_with("<!-- cartograph:end -->\nafter\n"));
        assert!(!updated.contains("stale"));
    }
}
