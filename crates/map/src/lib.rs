//! Local map server and embedded frontend.
//!
//! [`serve`] runs an offline HTTP server over a graph. The frontend is built once and embedded;
//! the JSON endpoints return only the level on screen, so the graph can be any size.

mod arrows;
mod assets;
mod describe_on_open;
mod error;
mod http;
mod labels;
mod level;
mod node;
mod node_id;
mod search;
mod server;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

pub use describe::{Backend, ProcessBackend};
pub use error::{Error, Result};
pub use server::{MapServer, SharedBackend};

/// Serves the map of the graph in `store_path` (a `.cartograph/` directory) on `bind` until the
/// process is stopped. Descriptions opened in the map are written with `claude -p`; use
/// [`MapServer`] to pick another [`Backend`].
pub fn serve(store_path: &Path, bind: SocketAddr) -> Result<()> {
    let backend: SharedBackend = Arc::new(ProcessBackend::claude());
    MapServer::bind(store_path, bind, backend)?.run()
}
