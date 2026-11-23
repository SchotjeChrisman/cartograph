//! How a component reads to a person: its description with the person's overrides laid over it.

use describe::Overrides;
use graph::{Component, Reader, UnixSeconds};

use crate::error::Result;

pub struct Labels {
    pub name: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub written_at: Option<UnixSeconds>,
    /// A description exists and was written for an older version of the component's code.
    pub description_stale: bool,
}

impl Labels {
    /// Code changed since the component was clustered, or since its description was written.
    pub fn is_stale(&self, component: &Component) -> bool {
        component.stale || self.description_stale
    }
}

/// Without a description the generated component name stands in and `summary` stays empty.
pub fn labels(reader: &Reader, overrides: &Overrides, component: &Component) -> Result<Labels> {
    let Some(description) = reader.description(component.id)? else {
        return Ok(Labels {
            name: component.name.clone(),
            summary: None,
            tags: Vec::new(),
            written_at: None,
            description_stale: false,
        });
    };
    let description_stale = description.is_stale_for(component);
    let written_at = description.written_at;
    let shown = overrides.apply(&component.name, description);
    Ok(Labels {
        name: shown.name,
        summary: Some(shown.summary),
        tags: shown.tags,
        written_at: Some(written_at),
        description_stale,
    })
}
