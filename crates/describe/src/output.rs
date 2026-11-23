//! Validating what the model answered: a JSON object with a name, a paragraph and tags.

use std::fmt;

use serde::Deserialize;

const MAX_NAME_CHARS: usize = 60;
const MAX_PARAGRAPH_CHARS: usize = 1200;
const MAX_TAGS: usize = 8;
const MAX_TAG_CHARS: usize = 32;

/// A description as the model wrote it, checked and trimmed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Generated {
    pub name: String,
    pub paragraph: String,
    pub tags: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OutputError {
    /// No `{ ... }` object anywhere in the reply.
    NoJson,
    NotADescription(String),
    Invalid(String),
}

impl fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputError::NoJson => write!(formatter, "the model reply holds no JSON object"),
            OutputError::NotADescription(message) => {
                write!(formatter, "the model reply is not a description: {message}")
            }
            OutputError::Invalid(message) => write!(formatter, "invalid description: {message}"),
        }
    }
}

impl std::error::Error for OutputError {}

/// Parses a reply, tolerating prose or a code fence around the object, which models add even
/// when told not to.
pub fn parse_reply(reply: &str) -> Result<Generated, OutputError> {
    let start = reply.find('{').ok_or(OutputError::NoJson)?;
    let end = reply.rfind('}').ok_or(OutputError::NoJson)?;
    if end < start {
        return Err(OutputError::NoJson);
    }
    let generated: Generated = serde_json::from_str(&reply[start..=end])
        .map_err(|error| OutputError::NotADescription(error.to_string()))?;
    validate(generated)
}

fn validate(generated: Generated) -> Result<Generated, OutputError> {
    let name = single_line("name", &generated.name, MAX_NAME_CHARS)?;
    let paragraph = generated.paragraph.trim().to_string();
    if paragraph.is_empty() || paragraph.chars().count() > MAX_PARAGRAPH_CHARS {
        return Err(OutputError::Invalid(format!(
            "paragraph must be 1 to {MAX_PARAGRAPH_CHARS} characters"
        )));
    }
    if generated.tags.is_empty() || generated.tags.len() > MAX_TAGS {
        return Err(OutputError::Invalid(format!("give 1 to {MAX_TAGS} tags")));
    }
    let tags = generated
        .tags
        .iter()
        .map(|tag| single_line("tag", tag, MAX_TAG_CHARS))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Generated {
        name,
        paragraph,
        tags,
    })
}

fn single_line(what: &str, text: &str, max_chars: usize) -> Result<String, OutputError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max_chars || text.contains('\n') {
        return Err(OutputError::Invalid(format!(
            "{what} must be one line of 1 to {max_chars} characters"
        )));
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"name": "Graph store", "paragraph": "Keeps the graph.", "tags": ["storage", "sqlite"]}"#;

    #[test]
    fn a_bare_object_parses() {
        let generated = parse_reply(VALID).unwrap();
        assert_eq!(generated.name, "Graph store");
        assert_eq!(generated.tags, ["storage", "sqlite"]);
    }

    #[test]
    fn a_fenced_object_with_prose_parses() {
        let reply = format!("Here you go:\n```json\n{VALID}\n```\nHope that helps.");
        assert_eq!(parse_reply(&reply).unwrap().name, "Graph store");
    }

    #[test]
    fn values_are_trimmed() {
        let reply = r#"{"name": "  A  ", "paragraph": " B ", "tags": [" c "]}"#;
        let generated = parse_reply(reply).unwrap();
        assert_eq!(
            (
                generated.name.as_str(),
                generated.paragraph.as_str(),
                generated.tags[0].as_str()
            ),
            ("A", "B", "c")
        );
    }

    #[test]
    fn replies_without_an_object_are_rejected() {
        assert_eq!(parse_reply("I cannot do that."), Err(OutputError::NoJson));
        assert_eq!(parse_reply("} {"), Err(OutputError::NoJson));
    }

    #[test]
    fn missing_fields_and_wrong_types_are_rejected() {
        for reply in [
            r#"{"name": "A", "tags": ["x"]}"#,
            r#"{"name": "A", "paragraph": "B", "tags": "x"}"#,
            r#"{"name": 3, "paragraph": "B", "tags": ["x"]}"#,
        ] {
            assert!(
                matches!(parse_reply(reply), Err(OutputError::NotADescription(_))),
                "{reply}"
            );
        }
    }

    #[test]
    fn empty_oversized_and_multiline_values_are_rejected() {
        let long_name = "n".repeat(MAX_NAME_CHARS + 1);
        let many_tags = ["t"; MAX_TAGS + 1].join("\",\"");
        for reply in [
            r#"{"name": " ", "paragraph": "B", "tags": ["x"]}"#.to_string(),
            r#"{"name": "A", "paragraph": "", "tags": ["x"]}"#.to_string(),
            r#"{"name": "A", "paragraph": "B", "tags": []}"#.to_string(),
            r#"{"name": "A", "paragraph": "B", "tags": ["x\ny"]}"#.to_string(),
            format!(r#"{{"name": "{long_name}", "paragraph": "B", "tags": ["x"]}}"#),
            format!(r#"{{"name": "A", "paragraph": "B", "tags": ["{many_tags}"]}}"#),
        ] {
            assert!(
                matches!(parse_reply(&reply), Err(OutputError::Invalid(_))),
                "{reply}"
            );
        }
    }
}
