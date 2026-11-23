use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Child;
use std::time::{Duration, Instant};

use crate::common::{TestDir, add_function, command, indexed_project, ok};

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

#[test]
fn map_serves_the_frontend_until_killed() {
    let project = indexed_project("map");
    ok(project.path(), &["cluster"]);
    let port = free_port();
    let server = KillOnDrop(
        command(project.path())
            .args(["map", "--bind", &format!("127.0.0.1:{port}")])
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let response = loop {
        if let Some(response) = get(port, "/") {
            break response;
        }
        assert!(Instant::now() < deadline, "the map server never answered");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        response.starts_with("HTTP/1.") && response.contains(" 200 OK"),
        "{response}"
    );
    drop(server);
}

#[test]
fn editing_code_greys_out_the_components_that_hold_it() {
    let project = indexed_project("map-stale");
    ok(project.path(), &["cluster"]);
    let port = free_port();
    let level = |project: &TestDir| {
        let server = KillOnDrop(
            command(project.path())
                .args(["map", "--bind", &format!("127.0.0.1:{port}")])
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let response = loop {
            if let Some(response) = get(port, "/api/level") {
                break response;
            }
            assert!(Instant::now() < deadline, "the map server never answered");
            std::thread::sleep(Duration::from_millis(100));
        };
        drop(server);
        response
    };
    assert!(level(&project).contains("\"stale\":false"));
    assert!(!level(&project).contains("\"stale\":true"));

    add_function(
        &project,
        "crates/core_lib/src/util.rs",
        "brand_new_function",
    );
    ok(project.path(), &["index"]);
    assert!(level(&project).contains("\"stale\":true"));
}
