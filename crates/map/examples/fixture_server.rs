//! Serves the test fixture graph on the given address, for the headless browser check.
//! Usage: `cargo run -p map --example fixture_server -- 127.0.0.1:7788`

#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use describe::FakeBackend;
use map::MapServer;

fn main() {
    let bind = std::env::args()
        .nth(1)
        .expect("usage: fixture_server <addr:port>");
    let scratch = common::Scratch::new("browser");
    common::write_fixture(&scratch.dir());
    let backend = Arc::new(FakeBackend::replying(common::REPLY));
    let server = MapServer::bind(scratch.dir().path(), bind.parse().unwrap(), backend).unwrap();
    println!("serving the fixture on http://{}", server.local_addr());
    server.run();
}
