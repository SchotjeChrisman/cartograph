//! Where a graph lives: `.cartograph/` inside the directory root.

use std::path::{Path, PathBuf};

const GRAPH_DIR_NAME: &str = ".cartograph";
const DATABASE_FILE: &str = "graph.db";
const LOCK_FILE: &str = "indexer.lock";
const EXTRACTION_CACHE_FILE: &str = "extraction-cache.db";

/// The `.cartograph/` directory of one root.
#[derive(Debug, Clone)]
pub struct GraphDir {
    path: PathBuf,
}

impl GraphDir {
    /// The graph directory of `root`; nothing is created yet.
    pub fn of_root(root: &Path) -> Self {
        GraphDir {
            path: root.join(GRAPH_DIR_NAME),
        }
    }

    /// A graph directory at an explicit path, for tests and tools.
    pub fn at(path: PathBuf) -> Self {
        GraphDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn database_path(&self) -> PathBuf {
        self.path.join(DATABASE_FILE)
    }

    pub fn extraction_cache_path(&self) -> PathBuf {
        self.path.join(EXTRACTION_CACHE_FILE)
    }

    pub fn lock_path(&self) -> PathBuf {
        self.path.join(LOCK_FILE)
    }

    pub fn create(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)
    }
}
