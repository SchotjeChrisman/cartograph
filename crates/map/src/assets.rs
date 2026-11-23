//! The built frontend, embedded in the binary so the map needs no network and no files.

use rust_embed::RustEmbed;

use crate::http::Reply;

/// The workspace enables rust-embed's `debug-embed`, so debug builds embed the bundle like release
/// builds do instead of reading `frontend/dist` from the compile-time path, which does not exist
/// wherever the binary runs from another machine or container. A tree without a built bundle
/// fails to compile and the error names the folder: run `npm run build` in `crates/map/frontend`.
#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Bundle;

const INDEX: &str = "index.html";

/// The embedded file at a URL path; `/` is the index page.
pub fn asset(path: &str) -> Option<Reply> {
    let name = match path.trim_start_matches('/') {
        "" => INDEX,
        other => other,
    };
    let file = Bundle::get(name)?;
    Some(Reply {
        status: 200,
        content_type: content_type_of(name).to_string(),
        body: file.data.into_owned(),
    })
}

/// The bundle holds only what the build emits, so a few extensions cover it.
fn content_type_of(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
