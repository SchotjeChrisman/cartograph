//! Scratch projects and a handle on the real `cartograph` binary.
#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(name: &str) -> Self {
        let base = std::env::var_os("TEST_SCRATCH")
            .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
        Self::in_directory(&base, name)
    }

    /// Under the target directory, which (unlike a tmpfs scratch) can hold executables.
    pub fn executable(name: &str) -> Self {
        Self::in_directory(Path::new(env!("CARGO_TARGET_TMPDIR")), name)
    }

    fn in_directory(base: &Path, name: &str) -> Self {
        let path = base.join(format!("cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test dir");
        TestDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, relative: &str, content: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.0.join(relative)).unwrap()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The `cartograph` binary, run in `dir` with a home directory that is not the real one.
pub fn command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cartograph"));
    command.current_dir(dir).env("HOME", dir.join(".test-home"));
    command
}

pub fn run(dir: &Path, args: &[&str]) -> Output {
    command(dir).args(args).output().expect("run cartograph")
}

/// Runs and requires success; returns stdout.
pub fn ok(dir: &Path, args: &[&str]) -> String {
    let output = run(dir, args);
    assert!(
        output.status.success(),
        "cartograph {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Backdates every file under `dir` so later edits are plainly newer than the graph.
pub fn age_files(dir: &Path) {
    let old = SystemTime::now() - Duration::from_secs(3600);
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            age_files(&path);
        } else {
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The index crate's Rust fixture plus a Python script the graph cannot parse, backdated.
pub fn rust_project(name: &str) -> TestDir {
    let project = TestDir::new(name);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../index/tests/fixtures/rust");
    copy_tree(&fixture, project.path());
    project.write("scripts/tool.py", "print('hi')\n");
    age_files(project.path());
    project
}

/// `rust_project`, indexed.
pub fn indexed_project(name: &str) -> TestDir {
    let project = rust_project(name);
    ok(project.path(), &["index"]);
    project
}

/// Adds a function to a file of the fixture and leaves its modification time at now.
pub fn add_function(project: &TestDir, relative: &str, name: &str) {
    let mut text = project.read(relative);
    text.push_str(&format!("\npub fn {name}() {{}}\n"));
    project.write(relative, &text);
}
