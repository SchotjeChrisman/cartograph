use std::path::Path;

use graph::{GraphDir, Reader, Writer};
use serde_json::json;

use crate::Result;
use crate::args::ClusterArgs;

pub fn run(root: &Path, args: &ClusterArgs, as_json: bool) -> Result<String> {
    let dir = GraphDir::of_root(root);
    let reader = Reader::open(&dir)?;
    let mut writer = Writer::open(&dir)?;
    let config = cluster::Config {
        include_vendored: args.include_vendored,
        ..cluster::Config::default()
    };
    let tree = cluster::recluster(&reader, &mut writer, &config)?;
    let components = tree.components.len();
    let files = tree.file_components.len();
    Ok(if as_json {
        serde_json::to_string(&json!({ "components": components, "files": files }))?
    } else {
        format!("clustered {files} files into {components} components")
    })
}
