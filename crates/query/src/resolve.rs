//! Turning the text an agent typed into one node, or into candidates to choose from.

use graph::{NodeInfo, Query, split_words};

use crate::budget::Fitter;
use crate::engine::{Engine, fit, fixed_text, more};
use crate::error::Result;
use crate::render;
use crate::report::{Candidates, Report};
use crate::request::{Find, Page};
use crate::source::ComponentSource;

/// More than a reader would pick between, fewer than a pool worth ranking.
const RESOLVE_LIMIT: usize = 20;

pub(crate) enum Resolved {
    One(NodeInfo),
    Ambiguous(Vec<NodeInfo>),
    Missing,
}

impl<S: Query + ComponentSource> Engine<'_, S> {
    /// One node when the text is unambiguous: a unique `@id`, a `path:line` (the smallest
    /// enclosing definition), a unique qualified name, or a name exactly one node has.
    pub(crate) fn resolve(&self, text: &str) -> Result<Resolved> {
        let mut found = self.source.find(text, RESOLVE_LIMIT)?;
        if found.len() <= 1 || is_location(text) {
            return Ok(match found.is_empty() {
                true => Resolved::Missing,
                false => Resolved::One(found.swap_remove(0)),
            });
        }
        let wanted = split_words(text);
        let mut exact = found
            .iter()
            .filter(|node| split_words(&node.name) == wanted);
        if let (Some(only), None) = (exact.next(), exact.next()) {
            let only = only.id;
            return Ok(Resolved::One(
                found
                    .into_iter()
                    .find(|node| node.id == only)
                    .expect("taken from found"),
            ));
        }
        Ok(Resolved::Ambiguous(found))
    }

    /// The answer to a tool that needed one node but got none or several.
    pub(crate) fn unresolved_report(
        &self,
        text: &str,
        resolved: Resolved,
        tool: &str,
        budget: usize,
        notes: &[String],
    ) -> Result<Report> {
        let Resolved::Ambiguous(pool) = resolved else {
            return Ok(Report::NotFound {
                query: text.to_string(),
            });
        };
        let page = Page { budget, after: 0 };
        let complete = pool.len() < RESOLVE_LIMIT;
        let candidates = self.candidates(text, pool, page, complete, Some(tool), notes)?;
        Ok(Report::Ambiguous(candidates))
    }

    /// Ranked candidates cut at the page's budget. `pool_is_complete` is false when the search
    /// stopped at its own cap, so the count left over is only a lower bound.
    pub(crate) fn candidates(
        &self,
        query: &str,
        pool: Vec<NodeInfo>,
        page: Page,
        pool_is_complete: bool,
        ambiguous_for: Option<&str>,
        notes: &[String],
    ) -> Result<Candidates> {
        let header = render::candidates_header(query, ambiguous_for);
        let mut fitter = Fitter::new(page.budget, &[&fixed_text(notes, &header)]);
        let pool_size = pool.len();
        let candidates = fit(
            pool.into_iter().skip(page.after),
            &mut fitter,
            |node| self.node_ref(node),
            |candidate| render::node_line(candidate, true),
        )?;
        let consumed = page.after + candidates.len();
        let next = Find {
            text: query.to_string(),
            page: Page {
                after: consumed,
                ..page
            },
        };
        Ok(Candidates {
            query: query.to_string(),
            more: more(
                pool_size.saturating_sub(consumed),
                pool_is_complete,
                next.command(),
            ),
            candidates,
            ambiguous_for: ambiguous_for.map(str::to_string),
        })
    }
}

fn is_location(text: &str) -> bool {
    text.rsplit_once(':')
        .is_some_and(|(_, line)| line.parse::<u32>().is_ok())
}
