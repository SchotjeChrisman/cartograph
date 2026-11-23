//! `coverage`: what was parsed, what was not, and how many calls resolved.

use graph::{CoverageInputs, EdgeKind, Query};

use crate::budget::Fitter;
use crate::engine::{Engine, fit, fixed_text, more};
use crate::error::Result;
use crate::render;
use crate::report::{CoverageReport, Report};
use crate::request::{Coverage, Page};
use crate::source::ComponentSource;

/// Problem files read past the page start.
const PROBLEM_FILE_FETCH: usize = 500;

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn coverage(&self, request: &Coverage, notes: &[String]) -> Result<Report> {
        let wanted = request.page.after + PROBLEM_FILE_FETCH;
        let mut inputs = self.source.coverage(wanted)?;
        let call_resolution = call_resolution(&inputs);
        let problem_files = std::mem::take(&mut inputs.problem_files);

        let summary = render::coverage_summary(&inputs, call_resolution);
        let mut fitter = Fitter::new(request.page.budget, &[&fixed_text(notes, &summary)]);
        let shown = fit(
            problem_files.into_iter().skip(request.page.after),
            &mut fitter,
            Ok,
            render::problem_file_line,
        )?;
        let consumed = request.page.after + shown.len();
        let remaining = usize::try_from(inputs.problem_file_total)
            .unwrap_or(usize::MAX)
            .saturating_sub(consumed);
        let next = Coverage {
            page: Page {
                after: consumed,
                ..request.page
            },
        };
        inputs.problem_files = shown;
        Ok(Report::Coverage(CoverageReport {
            more: more(remaining, true, next.command()),
            inputs,
            call_resolution,
        }))
    }
}

/// Share of call edges that reached a definition; `None` when the graph has no calls.
fn call_resolution(inputs: &CoverageInputs) -> Option<f64> {
    let calls: u64 = inputs
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .map(|edge| edge.count)
        .sum();
    (calls > 0).then(|| 1.0 - inputs.unresolved_edges.min(calls) as f64 / calls as f64)
}
