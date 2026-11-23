//! `explore`: the neighbourhood of a node, nearest first, to a chosen depth.

use std::collections::HashSet;

use graph::{ComponentId, Direction, EdgeKind, Neighbour, NodeInfo, Query, SymbolId};

use crate::budget::Fitter;
use crate::engine::{Engine, fit, fixed_text, more, rank_key};
use crate::error::Result;
use crate::render;
use crate::report::{EdgeSummary, ExploreReport, Related, Report};
use crate::request::{Explore, Page, Relation};
use crate::resolve::Resolved;
use crate::source::ComponentSource;

/// Neighbours read from the focus; the rest are counted in the footer only.
const FIRST_LEVEL_LIMIT: usize = 1000;
/// Neighbours read from each node past the first level, so depth stays affordable.
const DEEPER_LEVEL_LIMIT: usize = 25;
/// Total nodes gathered before ranking stops the walk.
const WALK_CAP: usize = 2000;

struct Found {
    neighbour: Neighbour,
    depth: u32,
    via: Option<SymbolId>,
}

struct Walk {
    found: Vec<Found>,
    /// Direct neighbours that were not read, so the footer count is a lower bound.
    unread: usize,
}

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn explore(&self, request: &Explore, notes: &[String]) -> Result<Report> {
        let focus = match self.resolve(&request.target)? {
            Resolved::One(node) => node,
            other => {
                let budget = request.page.budget;
                return self.unresolved_report(&request.target, other, "explore", budget, notes);
            }
        };
        let walk = self.walk(&focus, request)?;
        let focus = self.node_ref(focus)?;
        let total = walk.found.len() + walk.unread;
        let header = render::explore_header(&focus, request.relation, request.depth, total);
        let mut fitter = Fitter::new(request.page.budget, &[&fixed_text(notes, &header)]);
        let pool_size = walk.found.len();
        let related = fit(
            walk.found.into_iter().skip(request.page.after),
            &mut fitter,
            |found| self.related(found),
            render::related_line,
        )?;
        let consumed = request.page.after + related.len();
        let next = Explore {
            target: format!("@{}", focus.short_id),
            page: Page {
                after: consumed,
                ..request.page
            },
            ..request.clone()
        };
        Ok(Report::Explore(ExploreReport {
            more: more(
                pool_size.saturating_sub(consumed) + walk.unread,
                walk.unread == 0 && pool_size < WALK_CAP,
                next.command(),
            ),
            focus,
            relation: request.relation,
            depth: request.depth,
            total,
            related,
        }))
    }

    fn related(&self, found: Found) -> Result<Related> {
        let via = found.via.map(|id| self.short_id(id)).transpose()?;
        let Neighbour {
            node,
            direction,
            edge,
        } = found.neighbour;
        Ok(Related {
            node: self.node_ref(node)?,
            depth: found.depth,
            direction,
            edge: EdgeSummary {
                kind: edge.kind,
                provenance: edge.provenance,
                file: edge.file,
                line: edge.line,
                reason: edge.reason,
            },
            via,
        })
    }

    /// Breadth-first from the focus. Each level is ranked on its own, so nearer nodes always
    /// come before farther ones.
    fn walk(&self, focus: &NodeInfo, request: &Explore) -> Result<Walk> {
        let (direction, kinds) = edges_for(request.relation);
        let mut visited = HashSet::from([focus.id]);
        let mut frontier = vec![focus.id];
        let mut found: Vec<Found> = Vec::new();
        let mut unread = 0;
        for depth in 1..=request.depth {
            let limit = if depth == 1 {
                FIRST_LEVEL_LIMIT
            } else {
                DEEPER_LEVEL_LIMIT
            };
            let mut level = Vec::new();
            for parent in &frontier {
                let neighbours = self.source.neighbours(*parent, direction, kinds, limit)?;
                if depth == 1 {
                    unread = neighbours.total.saturating_sub(neighbours.items.len());
                }
                for neighbour in neighbours.items {
                    let wanted = in_component(&neighbour.node, request.component);
                    if wanted && visited.insert(neighbour.node.id) {
                        let via = (depth > 1).then_some(*parent);
                        level.push(Found {
                            neighbour,
                            depth,
                            via,
                        });
                    }
                }
            }
            level.sort_by_key(|item| rank_key(&item.neighbour.node, focus.component));
            frontier = level.iter().map(|item| item.neighbour.node.id).collect();
            found.extend(level);
            if found.len() >= WALK_CAP || frontier.is_empty() {
                break;
            }
        }
        found.truncate(WALK_CAP);
        Ok(Walk { found, unread })
    }
}

fn in_component(node: &NodeInfo, component: Option<ComponentId>) -> bool {
    component.is_none() || node.component == component
}

fn edges_for(relation: Relation) -> (Direction, &'static [EdgeKind]) {
    match relation {
        Relation::Callers => (Direction::In, &[EdgeKind::Calls]),
        Relation::Callees => (Direction::Out, &[EdgeKind::Calls]),
        Relation::Contains => (Direction::Out, &[EdgeKind::Contains]),
        Relation::All => (Direction::Both, &[]),
    }
}
