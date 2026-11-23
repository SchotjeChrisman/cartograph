//! Helpers shared by the integration tests: scratch trees, fixture copies and a text dump of a
//! whole graph that tests compare.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use graph::{EdgeKind, FileStatus, NodeKind, Provenance};
use index::{IndexOptions, IndexReport, index_directory};
use rusqlite::{Connection, OpenFlags};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(name: &str) -> Self {
        let base = std::env::var_os("TEST_SCRATCH")
            .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
        let path = base.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        TestDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, relative: &str, content: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    pub fn remove(&self, relative: &str) {
        std::fs::remove_file(self.0.join(relative)).unwrap();
    }

    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.0.join(relative)).unwrap()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies `tests/fixtures/<name>` into a scratch directory, then adds what a repository cannot
/// store: `.git` directories and a nested `.cartograph/`.
pub fn copy_fixture(name: &str, scratch: &str) -> TestDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let target = TestDir::new(scratch);
    copy_tree(&source, target.path());
    let compile_commands = target.path().join("compile_commands.json");
    if let Ok(text) = std::fs::read_to_string(&compile_commands) {
        let root = target.path().to_string_lossy().to_string();
        std::fs::write(&compile_commands, text.replace("/PROJECT_ROOT", &root)).unwrap();
    }
    if name == "cpp" {
        std::fs::create_dir_all(target.path().join(".git")).unwrap();
        std::fs::create_dir_all(target.path().join("third_party/zlib/.git")).unwrap();
        target.write("sub/.cartograph/graph.db", "not a database");
    }
    target
}

fn copy_tree(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

/// Copies a whole tree except the graph directory (its `config.toml` stays), for indexing from scratch.
pub fn copy_without_graph(source: &Path, scratch: &str) -> TestDir {
    let target = TestDir::new(scratch);
    copy_tree(source, target.path());
    let compile_commands = target.path().join("compile_commands.json");
    if let Ok(text) = std::fs::read_to_string(&compile_commands) {
        let moved = text.replace(
            &source.to_string_lossy().to_string(),
            &target.path().to_string_lossy(),
        );
        std::fs::write(&compile_commands, moved).unwrap();
    }
    let graph_dir = target.path().join(".cartograph");
    let config = std::fs::read(graph_dir.join("config.toml")).ok();
    let _ = std::fs::remove_dir_all(&graph_dir);
    if let Some(config) = config {
        std::fs::create_dir_all(&graph_dir).unwrap();
        std::fs::write(graph_dir.join("config.toml"), config).unwrap();
    }
    target
}

pub fn index(root: &Path) -> IndexReport {
    index_directory(
        root,
        &IndexOptions {
            batch_files: 7,
            now: 1_000,
        },
    )
    .expect("indexing succeeds")
}

/// Text form of the graph: files, nodes and edges, one per line, sorted. `with_hashes` adds the
/// content hash of each file.
pub fn dump(root: &Path, with_hashes: bool) -> String {
    let database = root.join(".cartograph/graph.db");
    let connection =
        Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let sections = [
        ("FILES", file_lines(&connection, with_hashes)),
        ("NODES", node_lines(&connection)),
        ("EDGES", edge_lines(&connection)),
    ];
    let mut text = String::new();
    for (title, mut lines) in sections {
        lines.sort();
        writeln!(text, "## {title}").unwrap();
        for line in lines {
            writeln!(text, "{line}").unwrap();
        }
    }
    text
}

fn optional(value: Option<String>) -> String {
    value.unwrap_or_else(|| "-".to_string())
}

fn file_lines(connection: &Connection, with_hashes: bool) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT path, repo, language, status, tags, content_hash, status_detail FROM files",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            let status = FileStatus::from_code(row.get(3)?).unwrap();
            let hash: Option<Vec<u8>> = row.get(5)?;
            let hash = match (with_hashes, hash) {
                (true, Some(bytes)) => format!(
                    " hash={}",
                    bytes
                        .iter()
                        .take(5)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
                _ => String::new(),
            };
            Ok(format!(
                "{} repo={} lang={} status={status} tags={}{hash} detail={}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                optional(row.get(2)?),
                row.get::<_, i64>(4)?,
                optional(row.get(6)?),
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn node_lines(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT n.kind, n.qualified_name, f.path, n.line, n.end_line, n.signature, n.doc
             FROM nodes n LEFT JOIN files f ON f.id = n.file_id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            let kind = NodeKind::from_code(row.get(0)?).unwrap();
            let line: Option<i64> = row.get(3)?;
            let end_line: Option<i64> = row.get(4)?;
            Ok(format!(
                "{kind} {} @{}:{}-{} sig={} doc={}",
                row.get::<_, String>(1)?,
                optional(row.get(2)?),
                line.map_or("-".to_string(), |line| line.to_string()),
                end_line.map_or("-".to_string(), |line| line.to_string()),
                optional(row.get(5)?),
                optional(
                    row.get::<_, Option<String>>(6)?
                        .map(|doc| doc.replace('\n', "\\n"))
                ),
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn edge_lines(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT sn.kind, sn.qualified_name, sf.path, e.kind, e.provenance,
                    dn.kind, dn.qualified_name, df.path, e.line, e.reason, ef.path
             FROM edges e
             LEFT JOIN nodes sn ON sn.id = e.src LEFT JOIN files sf ON sf.id = sn.file_id
             LEFT JOIN nodes dn ON dn.id = e.dst LEFT JOIN files df ON df.id = dn.file_id
             JOIN files ef ON ef.id = e.file_id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            let end = |kind: Option<i64>, name: Option<String>, file: Option<String>| match (
                kind, name,
            ) {
                (Some(kind), Some(name)) => {
                    format!(
                        "{} {name} @{}",
                        NodeKind::from_code(kind).unwrap(),
                        optional(file)
                    )
                }
                _ => "<MISSING NODE>".to_string(),
            };
            let src = end(row.get(0)?, row.get(1)?, row.get(2)?);
            let dst = end(row.get(5)?, row.get(6)?, row.get(7)?);
            let kind = EdgeKind::from_code(row.get(3)?).unwrap();
            let provenance = Provenance::from_code(row.get(4)?).unwrap();
            let line: Option<i64> = row.get(8)?;
            Ok(format!(
                "{src} -{kind}/{provenance}-> {dst} line={} by={} reason={}",
                line.map_or("-".to_string(), |line| line.to_string()),
                row.get::<_, String>(10)?,
                optional(row.get(9)?),
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Compares `actual` with `tests/golden/<name>.txt`; `UPDATE_GOLDEN=1` rewrites the file.
pub fn assert_golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden file {}; run with UPDATE_GOLDEN=1",
            path.display()
        )
    });
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let missing: Vec<&&str> = expected_lines
        .iter()
        .filter(|line| !actual_lines.contains(line))
        .collect();
    let unexpected: Vec<&&str> = actual_lines
        .iter()
        .filter(|line| !expected_lines.contains(line))
        .collect();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "{name}: graph differs from golden.\nmissing (expected, not produced):\n{}\nunexpected (produced, not expected):\n{}",
        missing
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        unexpected
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Indexes `tree` again incrementally, indexes a copy from scratch, and asserts the two graphs are identical.
pub fn assert_incremental_equals_full(tree: &TestDir, label: &str) -> IndexReport {
    let report = index(tree.path());
    let fresh = copy_without_graph(tree.path(), &format!("{label}-fresh"));
    index(fresh.path());
    let incremental = dump(tree.path(), true);
    let full = dump(fresh.path(), true);
    assert!(
        !incremental.contains("<MISSING NODE>"),
        "{label}: dangling edge in\n{incremental}"
    );
    if incremental != full {
        let left: Vec<&str> = incremental.lines().collect();
        let right: Vec<&str> = full.lines().collect();
        let only_incremental: Vec<&&str> =
            left.iter().filter(|line| !right.contains(line)).collect();
        let only_full: Vec<&&str> = right.iter().filter(|line| !left.contains(line)).collect();
        panic!(
            "{label}: incremental differs from full.\nonly incremental:\n{only_incremental:#?}\nonly full:\n{only_full:#?}"
        );
    }
    report
}
