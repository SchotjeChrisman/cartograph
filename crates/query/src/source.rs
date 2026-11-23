//! The component reads that [`graph::Query`] does not carry.

use graph::{Component, ComponentId, Description, Reader, Result};

/// Component rows and their descriptions, for naming components in output.
pub trait ComponentSource {
    fn component(&self, id: ComponentId) -> Result<Option<Component>>;
    fn description(&self, id: ComponentId) -> Result<Option<Description>>;
}

impl ComponentSource for Reader {
    fn component(&self, id: ComponentId) -> Result<Option<Component>> {
        Reader::component(self, id)
    }

    fn description(&self, id: ComponentId) -> Result<Option<Description>> {
        Reader::description(self, id)
    }
}
