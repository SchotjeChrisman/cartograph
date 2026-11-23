mod common;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use common::{REPLY, Scratch, write_fixture};
use describe::FakeBackend;
use map::MapServer;

/// Serves the fixture on a free local port with a fake describer, in a background thread.
pub fn serve_fixture(scratch: &Scratch, backend: FakeBackend) -> SocketAddr {
    write_fixture(&scratch.dir());
    let server = MapServer::bind(
        scratch.dir().path(),
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(backend),
    )
    .unwrap();
    let address = server.local_addr();
    std::thread::spawn(move || server.run());
    address
}

pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
}

pub fn request(address: SocketAddr, method: &str, target: &str) -> Response {
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    Response {
        status,
        body: body.to_string(),
    }
}

pub fn names(boxes: &serde_json::Value) -> HashMap<String, String> {
    boxes
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["id"].as_str().unwrap().to_string(),
                b["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn top_level_shows_systems_with_no_edges_between_them() {
    let scratch = Scratch::new("top");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let level = request(address, "GET", "/api/level").json();

    let boxes = names(&level["boxes"]);
    assert_eq!(boxes.get("c1").map(String::as_str), Some("app"));
    assert_eq!(boxes.get("c5").map(String::as_str), Some("tools"));
    assert!(level["arrows"].as_array().unwrap().is_empty());
    assert_eq!(level["breadcrumb"].as_array().unwrap().len(), 0);
    assert_eq!(level["freshness"]["indexed_at"], common::NOW);
}

#[test]
fn a_system_shows_its_children_with_weighted_arrows_and_staleness() {
    let scratch = Scratch::new("children");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let level = request(address, "GET", "/api/level?parent=c1").json();

    let boxes = level["boxes"].as_array().unwrap();
    let by_id = |id: &str| boxes.iter().find(|b| b["id"] == id).unwrap();
    assert_eq!(boxes.len(), 3);
    assert_eq!(by_id("c2")["name"], "Networking", "description name wins");
    assert_eq!(by_id("c2")["summary"], "Talks to the wire.");
    assert_eq!(by_id("c2")["file_count"], 2);
    assert_eq!(by_id("c3")["summary"], serde_json::Value::Null);
    assert_eq!(by_id("c4")["stale"], true);
    assert_eq!(by_id("c2")["stale"], false);
    let arrow = |from: &str, to: &str| {
        level["arrows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["from"] == from && a["to"] == to)
            .map(|a| a["count"].as_u64().unwrap())
    };
    assert_eq!(
        arrow("c3", "c2"),
        Some(2),
        "ui/a.rs calls net/a.rs and net/b.rs"
    );
    assert_eq!(arrow("c3", "c4"), Some(1));
    assert_eq!(arrow("c2", "c4"), Some(1));
    assert_eq!(arrow("c2", "c3"), None);
    assert_eq!(level["breadcrumb"][0]["id"], "c1");
}

#[test]
fn a_component_without_children_shows_its_files() {
    let scratch = Scratch::new("files");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let level = request(address, "GET", "/api/level?parent=c2").json();

    let kinds: Vec<_> = level["boxes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["kind"].as_str().unwrap(), b["name"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&("file", "net/a.rs")));
    let crumbs: Vec<_> = level["breadcrumb"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(crumbs, ["c1", "c2"]);
}

#[test]
fn node_detail_has_symbols_staleness_and_the_equivalent_command() {
    let scratch = Scratch::new("node");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let node = request(address, "GET", "/api/node?id=c2").json();

    assert_eq!(node["command"], "cartograph overview --component 2");
    assert_eq!(node["summary"], "Talks to the wire.");
    assert!(!node["key_symbols"].as_array().unwrap().is_empty());
    assert!(
        node["key_symbols"][0]["qualified_name"]
            .as_str()
            .unwrap()
            .contains("::f")
    );
    assert_eq!(node["description_stale"], false);
    let stale = request(address, "GET", "/api/node?id=c4").json();
    assert_eq!(stale["stale"], true);
}

#[test]
fn search_finds_components_and_symbols() {
    let scratch = Scratch::new("search");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let by_component = request(address, "GET", "/api/search?q=stor").json();
    let by_symbol = request(address, "GET", "/api/search?q=net_a_rs%3A%3Af1").json();
    let empty = request(address, "GET", "/api/search?q=%20").json();

    assert_eq!(by_component[0]["kind"], "component");
    assert_eq!(by_component[0]["target"], "c4");
    assert!(
        by_symbol
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["kind"] == "symbol" && hit["target"] == "c2"),
        "{by_symbol}"
    );
    assert!(empty.as_array().unwrap().is_empty());
}

#[test]
fn opening_an_undescribed_component_writes_its_description_once() {
    let scratch = Scratch::new("describe");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let first = request(address, "POST", "/api/describe?id=c3").json();
    let second = request(address, "POST", "/api/describe?id=c3").json();
    let level = request(address, "GET", "/api/level?parent=c1").json();

    assert_eq!(first["outcome"], "generated");
    assert_eq!(first["node"]["name"], "Storage layer");
    assert_eq!(first["node"]["summary"], "Keeps data on disk.");
    assert_eq!(second["outcome"], "cached");
    assert_eq!(names(&level["boxes"])["c3"], "Storage layer");
}

#[test]
fn a_failing_backend_is_reported_not_hidden() {
    let scratch = Scratch::new("describe-fail");
    let address = serve_fixture(&scratch, FakeBackend::failing("model offline"));

    let response = request(address, "POST", "/api/describe?id=c3");

    assert_eq!(response.status, 500);
    assert!(
        response.json()["error"]
            .as_str()
            .unwrap()
            .contains("model offline")
    );
}

#[test]
fn bad_requests_get_clear_statuses() {
    let scratch = Scratch::new("errors");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    assert_eq!(request(address, "GET", "/api/level?parent=zzz").status, 400);
    assert_eq!(request(address, "GET", "/api/level?parent=f1").status, 400);
    assert_eq!(request(address, "GET", "/api/level?parent=c99").status, 404);
    assert_eq!(request(address, "GET", "/api/node").status, 400);
    assert_eq!(request(address, "POST", "/api/describe?id=f1").status, 400);
    assert_eq!(request(address, "GET", "/api/nope").status, 404);
    assert_eq!(request(address, "POST", "/api/level").status, 405);
    assert_eq!(request(address, "GET", "/missing.js").status, 404);
}

#[test]
fn the_embedded_page_is_served_at_the_root() {
    let scratch = Scratch::new("assets");
    let address = serve_fixture(&scratch, FakeBackend::replying(REPLY));

    let page = request(address, "GET", "/");

    assert_eq!(page.status, 200);
    assert!(page.body.contains("<!doctype html>"));
}
