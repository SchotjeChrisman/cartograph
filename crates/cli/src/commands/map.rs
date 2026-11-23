use std::net::SocketAddr;
use std::path::Path;

use graph::{GraphDir, Reader};

use crate::Result;

/// Serves the map until the process is interrupted.
pub fn run(root: &Path, bind: SocketAddr) -> Result<()> {
    let dir = GraphDir::of_root(root);
    Reader::open(&dir)?;
    eprintln!(
        "cartograph map: serving {} on http://{bind}",
        root.display()
    );
    map::serve(dir.path(), bind)?;
    Ok(())
}
