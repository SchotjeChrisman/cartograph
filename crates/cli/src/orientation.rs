//! A marker file recording that some query has oriented the agent, which is what the strict hook
//! waits for.
//!
//! ponytail: one marker per graph, not per session, because queries do not know the agent's
//! session id; the hook honours it for a fixed window. Per-session markers need the session id
//! passed to `cartograph mcp`.

use std::io;
use std::time::{Duration, SystemTime};

use graph::GraphDir;

const MARKER_FILE: &str = "oriented";
/// How long after a query the hook keeps letting Read and Grep through.
pub const ORIENTATION_WINDOW: Duration = Duration::from_secs(30 * 60);

/// Records a query now by touching the marker.
pub fn mark(dir: &GraphDir) -> io::Result<()> {
    std::fs::write(dir.path().join(MARKER_FILE), b"")
}

/// Whether a query happened within the window before `now`.
pub fn is_recent(dir: &GraphDir, now: SystemTime) -> io::Result<bool> {
    let modified = match std::fs::metadata(dir.path().join(MARKER_FILE)) {
        Ok(metadata) => metadata.modified()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(now
        .duration_since(modified)
        .is_ok_and(|age| age <= ORIENTATION_WINDOW))
}
