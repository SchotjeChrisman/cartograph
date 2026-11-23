//! Flags stored components whose files changed after an index run, so the map greys them out and
//! descriptions are not trusted until the next `cluster` and `describe`.

use graph::{GraphDir, Reader, Writer};

/// Compares the stored components with the files now in the graph. A graph never clustered has
/// no components, so nothing is flagged.
pub fn mark(dir: &GraphDir, writer: &mut Writer) -> graph::Result<()> {
    let reader = Reader::open(dir)?;
    cluster::mark_stale(&reader, writer, &cluster::Config::default())?;
    Ok(())
}
