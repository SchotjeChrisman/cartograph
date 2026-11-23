//! `.cartograph/overrides.toml`: text written by a person, which always beats the model's.
//!
//! One table per component, keyed by the component's generated name (its directory path):
//!
//! ```toml
//! ["crates/graph"]
//! name = "Graph store"
//! summary = "Where every fact about the code lives."
//! tags = ["storage"]
//! ```
//!
//! Every field is optional; a field that is present replaces the model's.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use graph::{Description, GraphDir};
use serde::Deserialize;

const OVERRIDES_FILE: &str = "overrides.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub name: Option<String>,
    pub summary: Option<String>,
    pub tags: Option<Vec<String>>,
}

impl Override {
    /// Whether the person wrote every field, so the model has nothing left to add.
    pub fn is_complete(&self) -> bool {
        self.name.is_some() && self.summary.is_some() && self.tags.is_some()
    }
}

#[derive(Debug)]
pub enum OverridesError {
    Read {
        path: String,
        source: std::io::Error,
    },
    Parse(String),
    /// A text field that is present but empty, which would blank the component.
    Empty {
        component: String,
        field: &'static str,
    },
}

impl fmt::Display for OverridesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OverridesError::Read { path, source } => {
                write!(formatter, "cannot read {path}: {source}")
            }
            OverridesError::Parse(message) => {
                write!(formatter, "invalid {OVERRIDES_FILE}: {message}")
            }
            OverridesError::Empty { component, field } => write!(
                formatter,
                "{OVERRIDES_FILE}: `{field}` of [\"{component}\"] is empty; remove the key instead"
            ),
        }
    }
}

impl std::error::Error for OverridesError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides(HashMap<String, Override>);

impl Overrides {
    pub fn parse(text: &str) -> Result<Self, OverridesError> {
        let table: HashMap<String, Override> =
            toml::from_str(text).map_err(|error| OverridesError::Parse(error.to_string()))?;
        for (component, entry) in &table {
            let empty_field = [("name", &entry.name), ("summary", &entry.summary)]
                .into_iter()
                .find(|(_, value)| value.as_deref().is_some_and(|text| text.trim().is_empty()))
                .map(|(field, _)| field)
                .or_else(|| {
                    entry
                        .tags
                        .as_ref()
                        .is_some_and(|tags| tags.is_empty())
                        .then_some("tags")
                });
            if let Some(field) = empty_field {
                return Err(OverridesError::Empty {
                    component: component.clone(),
                    field,
                });
            }
        }
        Ok(Overrides(table))
    }

    /// Reads the graph directory's overrides; no file means no overrides.
    pub fn load(dir: &GraphDir) -> Result<Self, OverridesError> {
        Self::load_from(&dir.path().join(OVERRIDES_FILE))
    }

    fn load_from(path: &Path) -> Result<Self, OverridesError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(OverridesError::Read {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    pub fn for_component(&self, component_name: &str) -> Option<&Override> {
        self.0.get(component_name)
    }

    /// `description` with the person's fields laid over it. Readers call this on every
    /// description they show, so an override edited after the model ran still wins.
    pub fn apply(&self, component_name: &str, mut description: Description) -> Description {
        if let Some(entry) = self.for_component(component_name) {
            if let Some(name) = &entry.name {
                description.name = name.clone();
            }
            if let Some(summary) = &entry.summary {
                description.summary = summary.clone();
            }
            if let Some(tags) = &entry.tags {
                description.tags = tags.clone();
            }
        }
        description
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{ComponentId, ContentHash};

    fn description() -> Description {
        Description {
            component: ComponentId(1),
            component_hash: ContentHash::of_bytes(b"x"),
            name: "Model name".to_string(),
            summary: "Model summary".to_string(),
            tags: vec!["model".to_string()],
            written_at: 0,
        }
    }

    #[test]
    fn partial_overrides_replace_only_the_fields_present() {
        let overrides = Overrides::parse("[\"crates/graph\"]\nname = \"Graph store\"\n").unwrap();

        let applied = overrides.apply("crates/graph", description());

        assert_eq!(applied.name, "Graph store");
        assert_eq!(applied.summary, "Model summary");
        assert_eq!(applied.tags, ["model"]);
    }

    #[test]
    fn other_components_are_untouched() {
        let overrides = Overrides::parse("[\"a\"]\nname = \"A\"\n").unwrap();
        assert_eq!(overrides.apply("b", description()), description());
    }

    #[test]
    fn only_a_full_entry_is_complete() {
        let overrides = Overrides::parse(
            "[\"a\"]\nname = \"A\"\nsummary = \"S\"\ntags = [\"t\"]\n[\"b\"]\nname = \"B\"\n",
        )
        .unwrap();
        assert!(overrides.for_component("a").unwrap().is_complete());
        assert!(!overrides.for_component("b").unwrap().is_complete());
    }

    #[test]
    fn typos_and_blank_values_are_errors() {
        assert!(matches!(
            Overrides::parse("[\"a\"]\nsumary = \"x\"\n"),
            Err(OverridesError::Parse(_))
        ));
        assert!(matches!(
            Overrides::parse("[\"a\"]\nname = \"  \"\n"),
            Err(OverridesError::Empty { field: "name", .. })
        ));
        assert!(matches!(
            Overrides::parse("[\"a\"]\ntags = []\n"),
            Err(OverridesError::Empty { field: "tags", .. })
        ));
    }

    #[test]
    fn a_missing_file_means_no_overrides() {
        let missing = Path::new("/nonexistent-cartograph-dir/overrides.toml");
        assert_eq!(Overrides::load_from(missing).unwrap(), Overrides::default());
    }
}
