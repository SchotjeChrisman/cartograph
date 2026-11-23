//! `links.yaml`: edges a developer declares for what no parser can see, such as queues, cron
//! jobs, webhooks and routes registered at runtime. Each entry becomes a `declared` edge.
//!
//! ```yaml
//! links:
//!   - from: App\Jobs\SendInvoice::handle   # optional; left out, the edge starts at links.yaml
//!     to: App\Mail\InvoiceMail
//!     kind: queue                          # a label that explains the edge
//!     note: runs on the invoices queue     # optional
//! ```
//!
//! `from` and `to` name a file path, a qualified name or a short name from the graph. A name
//! that matches nothing, or several things, stays an unresolved end: visible in `coverage`.

mod resolve;
mod yaml;

use std::collections::{HashMap, HashSet};

use languages::{ProjectView, ResolvedEdge};

pub use yaml::parse;

/// Where declarations are looked for: files with this name, anywhere in the tree.
pub const LINKS_FILE: &str = "links.yaml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub from: Option<String>,
    pub to: String,
    pub kind: String,
    pub note: Option<String>,
    /// Line of the entry's first key, for messages and as the edge's line.
    pub line: u32,
}

/// The declared edges and the problems found, for every links file, keyed by its path. `files`
/// holds every file in the tree, parsed or not, since a link may start at a shell script.
pub fn declared_edges<'a>(
    link_files: impl IntoIterator<Item = (&'a str, &'a [Link])>,
    files: &HashSet<&str>,
    project: &ProjectView<'_>,
) -> (HashMap<String, Vec<ResolvedEdge>>, Vec<String>) {
    let mut edges = HashMap::new();
    let mut problems = Vec::new();
    for (path, links) in link_files {
        let declared = resolve::resolve(path, links, files, project);
        problems.extend(
            declared
                .problems
                .into_iter()
                .map(|problem| format!("{path}: {problem}")),
        );
        edges.insert(path.to_string(), declared.edges);
    }
    (edges, problems)
}
