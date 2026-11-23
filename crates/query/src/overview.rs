//! `overview`: the components of the system, level by level, with the symbols to start from.

use graph::{Component, ComponentId, Query};

use crate::budget::Fitter;
use crate::engine::{Engine, fit, fixed_text, more};
use crate::error::Result;
use crate::render;
use crate::report::{ComponentView, OverviewReport, Report};
use crate::request::{Overview, Page};
use crate::source::ComponentSource;

/// Most connected symbols listed under a component being drilled into.
const KEY_SYMBOL_COUNT: usize = 10;

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn overview(&self, request: &Overview, notes: &[String]) -> Result<Report> {
        let parent = match request.component {
            Some(id) => match self.source.component(id)? {
                Some(component) => Some(self.component_view(component)?),
                None => {
                    return Ok(Report::NotFound {
                        query: format!("component {}", id.0),
                    });
                }
            },
            None => None,
        };
        let children = self.source.components(request.component)?;
        let header = render::overview_header(parent.as_ref(), children.len());
        let mut fitter = Fitter::new(request.page.budget, &[&fixed_text(notes, &header)]);
        let child_count = children.len();
        let components = fit(
            children.into_iter().skip(request.page.after),
            &mut fitter,
            |component| self.component_view(component),
            render::component_line,
        )?;
        let consumed = request.page.after + components.len();
        let all_children_shown = consumed >= child_count;
        let key_symbols = match request.component {
            Some(id) if all_children_shown => self.key_symbols(id, &mut fitter)?,
            _ => Vec::new(),
        };
        let next = Overview {
            page: Page {
                after: consumed,
                ..request.page
            },
            ..request.clone()
        };
        Ok(Report::Overview(OverviewReport {
            more: more(child_count.saturating_sub(consumed), true, next.command()),
            parent,
            components,
            key_symbols,
        }))
    }

    fn key_symbols(
        &self,
        id: ComponentId,
        fitter: &mut Fitter,
    ) -> Result<Vec<crate::report::NodeRef>> {
        fit(
            self.source.top_symbols(id, KEY_SYMBOL_COUNT)?,
            fitter,
            |node| self.node_ref(node),
            |symbol| render::node_line(symbol, true),
        )
    }

    fn component_view(&self, component: Component) -> Result<ComponentView> {
        let description = self.source.description(component.id)?;
        let description_stale = description
            .as_ref()
            .is_some_and(|description| description.is_stale_for(&component));
        Ok(ComponentView {
            component,
            description,
            description_stale,
        })
    }
}
