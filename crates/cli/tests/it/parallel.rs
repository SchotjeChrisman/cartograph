use std::process::{Child, Stdio};

use crate::common::{TestDir, add_function, age_files, command, ok};

const PROCESSES: usize = 30;
const FILES: usize = 150;

fn wide_project(name: &str) -> TestDir {
    let project = TestDir::new(name);
    project.write("Cargo.toml", "[workspace]\n");
    for number in 0..FILES {
        let body = format!("pub fn work_{number}() -> usize {{ {} }}\n", number);
        project.write(&format!("src/m{number}.rs"), &body);
    }
    project.write(
        "src/lib.rs",
        &(0..FILES)
            .map(|n| format!("pub mod m{n};\n"))
            .collect::<String>(),
    );
    age_files(project.path());
    ok(project.path(), &["index"]);
    project
}

#[test]
fn thirty_queries_on_a_stale_graph_index_exactly_once() {
    let project = wide_project("thirty");
    let dir = project.path();
    add_function(&project, "src/m3.rs", "added_while_stale");
    add_function(&project, "src/m4.rs", "also_added_while_stale");

    let children: Vec<Child> = (0..PROCESSES)
        .map(|_| {
            command(dir)
                .args(["find", "added_while_stale"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();

    for output in &outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let texts: Vec<String> = outputs
        .iter()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .collect();
    let indexers = texts
        .iter()
        .filter(|text| text.contains("re-indexed"))
        .count();
    assert_eq!(
        indexers, 1,
        "exactly one process re-indexes; outputs: {texts:#?}"
    );
    for text in texts.iter().filter(|text| !text.contains("re-indexed")) {
        let saw_new_graph = text.contains("m3.rs");
        let warned = text.contains("indexing in progress") || text.contains("files changed");
        assert!(
            saw_new_graph || warned,
            "a reader neither saw the new graph nor was told it is stale: {text}"
        );
    }

    let settled = ok(dir, &["find", "also_added_while_stale"]);
    assert!(
        settled.contains("m4.rs") && !settled.contains("note:") && !settled.contains("warning:"),
        "{settled}"
    );
}
