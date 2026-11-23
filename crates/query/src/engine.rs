//! The entry point: runs the staleness check, dispatches a request, and builds node references.

use graph::{ComponentId, NodeInfo, Query, SymbolId, UnixSeconds};

use crate::budget::Fitter;
use crate::error::Result;
use crate::fresh::{self, NoCheck, StalenessCheck};
use crate::report::{More, NodeRef, Output};
use crate::request::Request;
use crate::source::ComponentSource;

/// Shortest id prefix shown; longer only when needed to be unique.
const MIN_SHORT_ID_DIGITS: usize = 4;

pub struct Engine<'a, S> {
    pub(crate) source: &'a S,
    check: Box<dyn StalenessCheck + 'a>,
    now: UnixSeconds,
}

impl<'a, S: Query + ComponentSource> Engine<'a, S> {
    /// `now` is passed in so the engine reads no clock; it only dates the staleness warning.
    pub fn new(source: &'a S, now: UnixSeconds) -> Self {
        Engine {
            source,
            check: Box::new(NoCheck),
            now,
        }
    }

    /// Runs `check` before every query; index supplies it to re-index a few changed files inline.
    pub fn with_staleness_check(mut self, check: impl StalenessCheck + 'a) -> Self {
        self.check = Box::new(check);
        self
    }

    pub fn run(&self, request: &Request) -> Result<Output> {
        let staleness = self.check.check()?;
        let notes = fresh::notes(staleness, &self.source.freshness()?, self.now);
        let report = match request {
            Request::Overview(request) => self.overview(request, &notes)?,
            Request::Find(request) => self.find(request, &notes)?,
            Request::Explore(request) => self.explore(request, &notes)?,
            Request::Path(request) => self.path(request, &notes)?,
            Request::Impact(request) => self.impact(request, &notes)?,
            Request::Coverage(request) => self.coverage(request, &notes)?,
        };
        Ok(Output { notes, report })
    }

    /// Attaches the short id and component name to a node. The short id is lengthened until no
    /// other node starts with it, so `@<short_id>` always resolves back to this node.
    pub(crate) fn node_ref(&self, node: NodeInfo) -> Result<NodeRef> {
        let short_id = self.short_id(node.id)?;
        let component_name = match node.component {
            Some(component) => self.component_name(component)?,
            None => None,
        };
        Ok(NodeRef {
            short_id,
            node,
            component_name,
        })
    }

    pub(crate) fn component_name(&self, id: ComponentId) -> Result<Option<String>> {
        Ok(self.source.component(id)?.map(|component| component.name))
    }

    pub(crate) fn short_id(&self, id: SymbolId) -> Result<String> {
        let mut digits = MIN_SHORT_ID_DIGITS;
        loop {
            let short = id.short(digits);
            let sharing = self.source.find(&format!("@{short}"), 2)?;
            if sharing.len() <= 1 || short.len() < digits {
                return Ok(short);
            }
            digits += 1;
        }
    }
}

/// Builds and admits items one at a time, so only the items that are shown pay for their
/// short id and component lookups.
pub(crate) fn fit<T, R>(
    items: impl IntoIterator<Item = T>,
    fitter: &mut Fitter,
    mut build: impl FnMut(T) -> Result<R>,
    line: impl Fn(&R) -> String,
) -> Result<Vec<R>> {
    let mut kept = Vec::new();
    for item in items {
        let built = build(item)?;
        if !fitter.admit(&line(&built)) {
            break;
        }
        kept.push(built);
    }
    Ok(kept)
}

/// The footer, or `None` when nothing was cut.
pub(crate) fn more(remaining: usize, exact: bool, command: String) -> Option<More> {
    (remaining > 0).then_some(More {
        remaining,
        exact,
        command,
    })
}

/// The text that is printed whatever the cut, for sizing the budget.
pub(crate) fn fixed_text(notes: &[String], header: &str) -> String {
    format!("{}\n{header}", notes.join("\n"))
}

/// Ordering shared by every ranked list: vendored and generated code last, the focus's own
/// component first, then the most connected.
pub(crate) fn rank_key(
    node: &NodeInfo,
    focus_component: Option<ComponentId>,
) -> (bool, bool, std::cmp::Reverse<u32>) {
    let deprioritised = node.tags.vendored || node.tags.generated;
    let same_component = focus_component.is_some() && node.component == focus_component;
    (
        deprioritised,
        !same_component,
        std::cmp::Reverse(node.degree),
    )
}
