//! `path`: the shortest dependency chain from one node to another.

use graph::{DEPENDENCY_KINDS, Direction, Query};

use crate::budget::Fitter;
use crate::engine::{Engine, fixed_text, more};
use crate::error::Result;
use crate::render;
use crate::report::{HopRef, Omitted, PathReport, Report};
use crate::request::PathRequest;
use crate::resolve::Resolved;
use crate::source::ComponentSource;

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn path(&self, request: &PathRequest, notes: &[String]) -> Result<Report> {
        let from = match self.resolve(&request.from)? {
            Resolved::One(node) => node,
            other => {
                return self.unresolved_report(&request.from, other, "path", request.budget, notes);
            }
        };
        let to = match self.resolve(&request.to)? {
            Resolved::One(node) => node,
            other => {
                return self.unresolved_report(&request.to, other, "path", request.budget, notes);
            }
        };
        let hops = self.source.path(
            from.id,
            to.id,
            Direction::Out,
            DEPENDENCY_KINDS,
            request.max_depth,
        )?;
        let (from, to) = (self.node_ref(from)?, self.node_ref(to)?);
        let hops = hops
            .map(|hops| {
                hops.into_iter()
                    .map(|hop| {
                        Ok(HopRef {
                            node: self.node_ref(hop.node)?,
                            via: hop.via,
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let hop_count = hops.as_ref().map(|hops| hops.len().saturating_sub(1));
        let header = render::path_header(&from, &to, request.max_depth, hop_count);
        let mut fitter = Fitter::new(request.budget, &[&fixed_text(notes, &header)]);
        let (hops, omitted) = match hops {
            Some(hops) => {
                let (kept, omitted) = keep_ends(hops, &mut fitter);
                (Some(kept), omitted)
            }
            None => (None, None),
        };
        let next = PathRequest {
            from: format!("@{}", from.short_id),
            to: format!("@{}", to.short_id),
            budget: request.budget.saturating_mul(2),
            ..request.clone()
        };
        Ok(Report::Path(PathReport {
            more: omitted.and_then(|omitted| more(omitted.count, true, next.command())),
            from,
            to,
            max_depth: request.max_depth,
            hop_count,
            hops,
            omitted,
        }))
    }
}

/// Keeps hops from both ends inward until the budget is spent, so the start and the destination
/// survive and the middle is what gets elided.
fn keep_ends(hops: Vec<HopRef>, fitter: &mut Fitter) -> (Vec<HopRef>, Option<Omitted>) {
    let total = hops.len();
    let (mut front, mut back) = (0, 0);
    while front + back < total {
        let from_front = front <= back;
        let index = if from_front { front } else { total - 1 - back };
        if !fitter.admit(&render::hop_line(&hops[index])) {
            break;
        }
        if from_front {
            front += 1;
        } else {
            back += 1;
        }
    }
    let omitted = (front + back < total).then_some(Omitted {
        after_index: front,
        count: total - front - back,
    });
    let kept = hops
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index < front || *index >= total - back)
        .map(|(_, hop)| hop)
        .collect();
    (kept, omitted)
}
