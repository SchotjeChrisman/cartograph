//! The describer's input: a graph slice about one component, rendered as a prompt of bounded
//! size. File bodies never go in; the model sees names, signatures, doc lines and edges.

/// Default ceiling for one prompt, in tokens.
pub const DEFAULT_TOKEN_CAP: usize = 6000;

/// No tokenizer is available offline; four characters per token is the usual rough measure.
const CHARS_PER_TOKEN: usize = 4;
const MAX_DOC_CHARS: usize = 160;

const INSTRUCTIONS: &str = "You are documenting one part of a large codebase for people who have not read it. \
From the facts below, write what this component is for, in plain English, without code or file names unless they \
are the clearest label.\n\
Reply with only a JSON object, no other text: \
{\"name\": \"a short human name\", \"paragraph\": \"one paragraph of what it does and why it matters\", \
\"tags\": [\"one to eight short lowercase topics\"]}\n";

/// A symbol as the prompt shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLine {
    pub kind: String,
    pub qualified_name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub signature: Option<String>,
    pub doc: Option<String>,
}

/// How strongly the described component is wired to one of its siblings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentLink {
    pub name: String,
    /// Edges from the described component into the other one.
    pub outgoing: u64,
    pub incoming: u64,
}

/// A child component and what it was already described as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSummary {
    pub name: String,
    pub summary: String,
}

/// Everything the model is told about a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSlice {
    pub name: String,
    pub level: u32,
    pub file_count: usize,
    /// Most connected first.
    pub symbols: Vec<SymbolLine>,
    /// Symbols that code outside the component calls into.
    pub entry_points: Vec<SymbolLine>,
    /// Strongest first.
    pub links: Vec<ComponentLink>,
    pub children: Vec<ChildSummary>,
}

/// Renders the slice, dropping the lowest-priority lines once `token_cap` would be exceeded.
/// Priority is children, links, entry points, then symbols.
pub fn build_prompt(slice: &ComponentSlice, token_cap: usize) -> String {
    let header = format!(
        "{INSTRUCTIONS}\nComponent: {} (level {}, {} files)\n",
        slice.name, slice.level, slice.file_count
    );
    let mut remaining = (token_cap * CHARS_PER_TOKEN).saturating_sub(header.len());
    let sections = [
        (
            "Parts already described",
            slice.children.iter().map(child_line).collect::<Vec<_>>(),
        ),
        (
            "Strongest links to sibling components",
            slice.links.iter().map(link_line).collect(),
        ),
        (
            "Entry points (called from outside)",
            slice.entry_points.iter().map(symbol_line).collect(),
        ),
        (
            "Key symbols",
            slice.symbols.iter().map(symbol_line).collect(),
        ),
    ];
    let mut prompt = header;
    for (title, lines) in sections {
        if lines.is_empty() {
            continue;
        }
        let (section, used) = render_section(title, &lines, remaining);
        remaining = remaining.saturating_sub(used);
        prompt.push_str(&section);
    }
    prompt
}

/// The section's text and how many characters it used; lines that do not fit are counted, not shown.
fn render_section(title: &str, lines: &[String], budget: usize) -> (String, usize) {
    let mut text = format!("\n{title}:\n");
    for (index, line) in lines.iter().enumerate() {
        if text.len() + line.len() + 1 > budget {
            text.push_str(&format!("... {} more omitted\n", lines.len() - index));
            break;
        }
        text.push_str(line);
        text.push('\n');
    }
    let used = text.len();
    (text, used)
}

fn child_line(child: &ChildSummary) -> String {
    format!("- {}: {}", child.name, child.summary)
}

fn link_line(link: &ComponentLink) -> String {
    format!(
        "- {}: {} edges out, {} edges in",
        link.name, link.outgoing, link.incoming
    )
}

fn symbol_line(symbol: &SymbolLine) -> String {
    let mut line = format!("- {} {}", symbol.kind, symbol.qualified_name);
    if let Some(signature) = &symbol.signature {
        line.push_str(&format!(" -- {signature}"));
    }
    if let (Some(file), Some(number)) = (&symbol.file, symbol.line) {
        line.push_str(&format!(" ({file}:{number})"));
    }
    if let Some(doc) = symbol.doc.as_deref().and_then(first_doc_line) {
        line.push_str(&format!(" // {doc}"));
    }
    line
}

fn first_doc_line(doc: &str) -> Option<String> {
    let line = doc.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_DOC_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(index: usize) -> SymbolLine {
        SymbolLine {
            kind: "function".to_string(),
            qualified_name: format!("module::function_{index}"),
            file: Some("src/lib.rs".to_string()),
            line: Some(index as u32),
            signature: Some(format!("fn function_{index}(input: &str) -> bool")),
            doc: Some("\n  Checks the input.\nSecond line is dropped.".to_string()),
        }
    }

    fn slice(symbol_count: usize) -> ComponentSlice {
        ComponentSlice {
            name: "crates/graph".to_string(),
            level: 1,
            file_count: 12,
            symbols: (0..symbol_count).map(symbol).collect(),
            entry_points: vec![symbol(900)],
            links: vec![ComponentLink {
                name: "crates/query".to_string(),
                outgoing: 3,
                incoming: 40,
            }],
            children: vec![ChildSummary {
                name: "crates/graph/src".to_string(),
                summary: "Loads facts.".to_string(),
            }],
        }
    }

    #[test]
    fn the_prompt_shows_every_section_with_signatures_and_first_doc_lines() {
        let prompt = build_prompt(&slice(2), DEFAULT_TOKEN_CAP);

        assert!(prompt.contains("Component: crates/graph (level 1, 12 files)"));
        assert!(prompt.contains("- crates/graph/src: Loads facts."));
        assert!(prompt.contains("- crates/query: 3 edges out, 40 edges in"));
        assert!(prompt.contains("Entry points"));
        assert!(prompt.contains(
            "- function module::function_1 -- fn function_1(input: &str) -> bool (src/lib.rs:1) // Checks the input."
        ));
        assert!(!prompt.contains("Second line"));
    }

    #[test]
    fn a_huge_slice_stays_under_the_cap_and_says_what_was_cut() {
        let cap = 500;

        let prompt = build_prompt(&slice(10_000), cap);

        assert!(
            prompt.len() <= cap * CHARS_PER_TOKEN,
            "{} chars",
            prompt.len()
        );
        assert!(prompt.contains("more omitted"));
        assert!(
            prompt.contains("- crates/graph/src: Loads facts."),
            "high-priority lines survive"
        );
    }

    #[test]
    fn empty_sections_are_left_out() {
        let bare = ComponentSlice {
            symbols: vec![],
            entry_points: vec![],
            links: vec![],
            children: vec![],
            ..slice(0)
        };

        let prompt = build_prompt(&bare, DEFAULT_TOKEN_CAP);

        assert!(!prompt.contains("Key symbols"));
        assert!(!prompt.contains("Strongest links"));
    }

    #[test]
    fn the_prompt_asks_for_the_json_shape_the_parser_expects() {
        let prompt = build_prompt(&slice(0), DEFAULT_TOKEN_CAP);
        for key in ["\"name\"", "\"paragraph\"", "\"tags\""] {
            assert!(prompt.contains(key), "{key}");
        }
    }
}
