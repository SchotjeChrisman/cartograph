//! `impact`: everything that depends on a node, nearest first.

use std::collections::BTreeMap;

use graph::{DEPENDENCY_KINDS, Query};

use crate::budget::Fitter;
use crate::engine::{Engine, fit, fixed_text, more, rank_key};
use crate::error::Result;
use crate::render;
use crate::report::{ImpactReport, Reachable, Report};
use crate::request::{Impact, Page};
use crate::resolve::Resolved;
use crate::source::ComponentSource;

/// Dependents read from the graph; the rest are counted in the footer only.
const REACH_FETCH: usize = 2000;

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn impact(&self, request: &Impact, notes: &[String]) -> Result<Report> {
        let target = match self.resolve(&request.target)? {
            Resolved::One(node) => node,
            other => {
                let budget = request.page.budget;
                return self.unresolved_report(&request.target, other, "impact", budget, notes);
            }
        };
        let mut impact =
            self.source
                .impact(target.id, DEPENDENCY_KINDS, request.depth, REACH_FETCH)?;
        impact
            .reached
            .sort_by_key(|item| (item.depth, rank_key(&item.node, target.component)));
        let mut by_depth = BTreeMap::new();
        for item in &impact.reached {
            *by_depth.entry(item.depth).or_insert(0) += 1;
        }
        let by_depth: Vec<(u32, usize)> = by_depth.into_iter().collect();

        let target = self.node_ref(target)?;
        let header = render::impact_header(&target, impact.total, &by_depth, impact.truncated);
        let mut fitter = Fitter::new(request.page.budget, &[&fixed_text(notes, &header)]);
        let fetched = impact.reached.len();
        let reached = fit(
            impact.reached.into_iter().skip(request.page.after),
            &mut fitter,
            |item| {
                Ok(Reachable {
                    node: self.node_ref(item.node)?,
                    depth: item.depth,
                })
            },
            render::reachable_line,
        )?;
        let consumed = request.page.after + reached.len();
        let next = Impact {
            target: format!("@{}", target.short_id),
            page: Page {
                after: consumed,
                ..request.page
            },
            ..request.clone()
        };
        Ok(Report::Impact(ImpactReport {
            more: more(
                impact.total.saturating_sub(consumed),
                !impact.truncated && fetched == impact.total,
                next.command(),
            ),
            target,
            max_depth: request.depth,
            total: impact.total,
            by_depth,
            truncated: impact.truncated,
            reached,
        }))
    }
}
