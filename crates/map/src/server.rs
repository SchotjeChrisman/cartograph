//! Accepting connections and routing requests to the views.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use describe::{Backend, Overrides};
use graph::{GraphDir, Reader};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::assets::asset;
use crate::describe_on_open::describe_component;
use crate::error::{Error, Result};
use crate::http::{Reply, Target};
use crate::level::level;
use crate::node::node;
use crate::node_id::NodeId;
use crate::search::search;

/// The model backend runs on the request thread that asked for a description.
pub type SharedBackend = Arc<dyn Backend + Send + Sync>;

struct State {
    dir: GraphDir,
    backend: SharedBackend,
}

/// A bound listener that has not started answering yet.
pub struct MapServer {
    server: Server,
    state: Arc<State>,
}

impl MapServer {
    /// Binds `bind` and checks the graph opens; `store_path` is the `.cartograph/` directory.
    pub fn bind(store_path: &Path, bind: SocketAddr, backend: SharedBackend) -> Result<Self> {
        let dir = GraphDir::at(store_path.to_path_buf());
        Reader::open(&dir)?;
        let server = Server::http(bind).map_err(Error::Bind)?;
        Ok(MapServer {
            server,
            state: Arc::new(State { dir, backend }),
        })
    }

    /// The address actually bound, which differs from the requested one for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.server
            .server_addr()
            .to_ip()
            .expect("the server listens on an IP socket")
    }

    /// Answers requests until the process ends, one thread per request so a describe call that
    /// runs for minutes does not hold up the pictures.
    // ponytail: unbounded threads; a pool if a shared map ever sees many viewers.
    pub fn run(self) -> ! {
        for request in self.server.incoming_requests() {
            let state = Arc::clone(&self.state);
            thread::spawn(move || answer(&state, request));
        }
        unreachable!("incoming_requests only ends when the server is dropped");
    }
}

fn answer(state: &State, request: Request) {
    let reply = route(state, request.method(), &Target::parse(request.url()));
    let content_type = Header::from_bytes("Content-Type", reply.content_type.as_bytes())
        .expect("content types are plain ASCII");
    let response = Response::from_data(reply.body)
        .with_status_code(reply.status)
        .with_header(content_type);
    if let Err(error) = request.respond(response) {
        eprintln!("map: could not send a response: {error}");
    }
}

fn route(state: &State, method: &Method, target: &Target) -> Reply {
    match (method, target.path.as_str()) {
        (Method::Get, "/api/level") => api(|| level_reply(state, target)),
        (Method::Get, "/api/node") => api(|| node_reply(state, target)),
        (Method::Get, "/api/search") => api(|| search_reply(state, target)),
        (Method::Post, "/api/describe") => api(|| describe_reply(state, target)),
        (Method::Get, path) if !path.starts_with("/api/") => {
            asset(path).unwrap_or_else(Reply::not_found)
        }
        (_, path) if path.starts_with("/api/") && !is_known_api(path) => Reply::not_found(),
        _ => Reply::method_not_allowed(),
    }
}

fn is_known_api(path: &str) -> bool {
    ["/api/level", "/api/node", "/api/search", "/api/describe"].contains(&path)
}

fn api(handler: impl FnOnce() -> Result<Reply>) -> Reply {
    handler().unwrap_or_else(|error| Reply::error(&error))
}

fn level_reply(state: &State, target: &Target) -> Result<Reply> {
    let parent = target.param("parent").map(NodeId::parse).transpose()?;
    Reply::json(&level(
        &Reader::open(&state.dir)?,
        &Overrides::load(&state.dir)?,
        parent,
    )?)
}

fn node_reply(state: &State, target: &Target) -> Result<Reply> {
    let id = NodeId::parse(required(target, "id")?)?;
    Reply::json(&node(
        &Reader::open(&state.dir)?,
        &Overrides::load(&state.dir)?,
        id,
    )?)
}

fn search_reply(state: &State, target: &Target) -> Result<Reply> {
    Reply::json(&search(
        &Reader::open(&state.dir)?,
        target.param("q").unwrap_or_default(),
    )?)
}

fn describe_reply(state: &State, target: &Target) -> Result<Reply> {
    let id = NodeId::parse(required(target, "id")?)?;
    Reply::json(&describe_component(
        &state.dir,
        state.backend.as_ref(),
        &Overrides::load(&state.dir)?,
        id,
    )?)
}

fn required<'a>(target: &'a Target, key: &str) -> Result<&'a str> {
    target
        .param(key)
        .ok_or_else(|| Error::BadRequest(format!("missing `{key}`")))
}
