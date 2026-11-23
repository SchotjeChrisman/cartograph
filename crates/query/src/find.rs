//! `find`: resolve a name or question to ranked candidates.

use graph::Query;

use crate::engine::Engine;
use crate::error::Result;
use crate::report::Report;
use crate::request::Find;
use crate::source::ComponentSource;

/// Candidates fetched past the page start; a reader never pages through more than this.
const CANDIDATE_FETCH: usize = 200;

impl<S: Query + ComponentSource> Engine<'_, S> {
    pub(crate) fn find(&self, request: &Find, notes: &[String]) -> Result<Report> {
        let wanted = request.page.after + CANDIDATE_FETCH;
        let pool = self.source.find(&request.text, wanted)?;
        let complete = pool.len() < wanted;
        let candidates =
            self.candidates(&request.text, pool, request.page, complete, None, notes)?;
        Ok(Report::Find(candidates))
    }
}
