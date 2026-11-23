//! A generated mid-size tree (about 700 C++ files and 120 Rust modules): indexing finishes,
//! resolves what the generator made resolvable, and stays equal to a full index after a batch of
//! edits, additions and deletions.

use crate::common::{TestDir, assert_incremental_equals_full, dump, index};

const CPP_DIRS: usize = 12;
const CPP_MODULES_PER_DIR: usize = 25;
const RUST_CRATES: usize = 3;
const RUST_MODULES_PER_CRATE: usize = 40;
const FUNCTIONS_PER_MODULE: usize = 4;

/// Small deterministic generator, so a failing tree can be rebuilt exactly.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

fn cpp_module_path(index: usize) -> (String, String) {
    let dir = index / CPP_MODULES_PER_DIR;
    (
        format!("cpp/d{dir}/mod{index}.h"),
        format!("cpp/d{dir}/mod{index}.cpp"),
    )
}

fn write_cpp_module(tree: &TestDir, index: usize, random: &mut Lcg) {
    let (header_path, source_path) = cpp_module_path(index);
    let dependencies: Vec<usize> = if index == 0 {
        Vec::new()
    } else {
        (0..2).map(|_| random.below(index)).collect()
    };
    let namespace = format!("ns{index}");
    let mut header = "#pragma once\n".to_string();
    for dependency in &dependencies {
        let (dependency_header, _) = cpp_module_path(*dependency);
        header.push_str(&format!(
            "#include \"{}\"\n",
            dependency_header.trim_start_matches("cpp/")
        ));
    }
    header.push_str(&format!("namespace {namespace} {{\n"));
    for function in 0..FUNCTIONS_PER_MODULE {
        header.push_str(&format!("int f{function}(int x);\n"));
    }
    let base = dependencies
        .first()
        .map(|dependency| format!(" : public ns{dependency}::T{dependency}"))
        .unwrap_or_default();
    header.push_str(&format!(
        "class T{index}{base} {{\n public:\n  void go_{index}();\n}};\n}}\n"
    ));
    tree.write(&header_path, &header);

    let header_name = format!("mod{index}.h");
    let mut source =
        format!("#include \"{header_name}\"\n#include <cstdio>\n\nnamespace {namespace} {{\n");
    for function in 0..FUNCTIONS_PER_MODULE {
        source.push_str(&format!("int f{function}(int x) {{\n"));
        if function > 0 {
            source.push_str(&format!("  x += f{}(x);\n", function - 1));
        }
        if let Some(dependency) = dependencies.get(function % 2) {
            let target = random.below(FUNCTIONS_PER_MODULE);
            source.push_str(&format!("  x += ns{dependency}::f{target}(x);\n"));
        }
        source.push_str("  printf(\"%d\", x);\n  return x;\n}\n");
    }
    source.push_str(&format!("void T{index}::go_{index}() {{ f0(1); }}\n}}\n"));
    tree.write(&source_path, &source);
}

fn write_rust_crate(tree: &TestDir, krate: usize, random: &mut Lcg) {
    let root = format!("rust/rc{krate}");
    let dependency = if krate > 0 {
        format!(
            "\n[dependencies]\nrc{} = {{ path = \"../rc{}\" }}\n",
            krate - 1,
            krate - 1
        )
    } else {
        String::new()
    };
    tree.write(
        &format!("{root}/Cargo.toml"),
        &format!("[package]\nname = \"rc{krate}\"\nversion = \"0.1.0\"\n{dependency}"),
    );
    let modules: String = (0..RUST_MODULES_PER_CRATE)
        .map(|module| format!("pub mod m{module};\n"))
        .collect();
    tree.write(&format!("{root}/src/lib.rs"), &modules);
    for module in 0..RUST_MODULES_PER_CRATE {
        write_rust_module(tree, krate, module, random);
    }
}

fn write_rust_module(tree: &TestDir, krate: usize, module: usize, random: &mut Lcg) {
    let mut source = String::new();
    if module > 0 {
        let used = random.below(module);
        source.push_str(&format!("use crate::m{used}::f0 as imported;\n"));
    }
    if krate > 0 {
        source.push_str(&format!(
            "use rc{}::m{}::S{};\n",
            krate - 1,
            module % RUST_MODULES_PER_CRATE,
            module
        ));
    }
    source.push_str(&format!("pub struct S{module};\n\nimpl S{module} {{\n    pub fn run_{module}(&self) {{ f0(); }}\n}}\n"));
    for function in 0..FUNCTIONS_PER_MODULE {
        source.push_str(&format!("pub fn f{function}() {{\n"));
        if function > 0 {
            source.push_str(&format!("    f{}();\n", function - 1));
        }
        if module > 0 {
            let other = random.below(module);
            source.push_str(&format!(
                "    crate::m{other}::f{}();\n",
                random.below(FUNCTIONS_PER_MODULE)
            ));
            if function == 0 {
                source.push_str("    imported();\n");
            }
        }
        source.push_str(&format!("    S{module}.run_{module}();\n}}\n"));
    }
    tree.write(&format!("rust/rc{krate}/src/m{module}.rs"), &source);
}

fn generate(name: &str) -> (TestDir, Lcg) {
    let tree = TestDir::new(name);
    let mut random = Lcg(42);
    for module in 0..CPP_DIRS * CPP_MODULES_PER_DIR {
        write_cpp_module(&tree, module, &mut random);
    }
    for krate in 0..RUST_CRATES {
        write_rust_crate(&tree, krate, &mut random);
    }
    (tree, random)
}

fn count(dump: &str, needle: &str) -> usize {
    dump.lines().filter(|line| line.contains(needle)).count()
}

#[test]
fn a_generated_tree_resolves_everything_the_generator_made_resolvable() {
    let (tree, _) = generate("synthetic-full");
    let report = index(tree.path());
    let cpp_modules = CPP_DIRS * CPP_MODULES_PER_DIR;
    let rust_files = RUST_CRATES * (RUST_MODULES_PER_CRATE + 1);
    assert_eq!(report.files, cpp_modules * 2 + rust_files);
    assert_eq!(report.unchanged, 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let graph = dump(tree.path(), false);
    assert!(!graph.contains("<MISSING NODE>"));
    let printf_calls = cpp_modules * FUNCTIONS_PER_MODULE;
    assert_eq!(
        count(&graph, "-calls/extracted-> unresolved"),
        printf_calls,
        "only the printf calls are unresolvable"
    );
    assert_eq!(count(&graph, "-imports/extracted-> unresolved"), 0);
    assert_eq!(count(&graph, "-inherits/extracted-> unresolved"), 0);
    assert!(count(&graph, "-calls/inferred->") > cpp_modules * FUNCTIONS_PER_MODULE);
}

#[test]
fn editing_bodies_re_parses_only_those_files_and_wakes_no_one() {
    let (tree, mut random) = generate("synthetic-edits");
    let first = index(tree.path());

    let cpp_modules = CPP_DIRS * CPP_MODULES_PER_DIR;
    let mut edited = 0;
    for _ in 0..8 {
        let module = random.below(cpp_modules);
        let (_, source_path) = cpp_module_path(module);
        let source = tree.read(&source_path);
        tree.write(&source_path, &format!("{source}// touched {edited}\n"));
        edited += 1;
    }
    for _ in 0..4 {
        let module = random.below(RUST_MODULES_PER_CRATE);
        let path = format!("rust/rc1/src/m{module}.rs");
        let source = tree.read(&path);
        tree.write(&path, &format!("{source}// touched {edited}\n"));
        edited += 1;
    }
    let report = assert_incremental_equals_full(&tree, "synthetic-edits-check");
    assert!(report.parsed <= edited && report.parsed > 0, "{report:?}");
    assert_eq!(report.re_resolved, 0, "{report:?}");
    assert_eq!(report.unchanged, first.files - report.parsed);
}

#[test]
fn incremental_equals_full_after_new_functions_new_files_and_deletions() {
    let (tree, mut random) = generate("synthetic-incremental");
    index(tree.path());

    let cpp_modules = CPP_DIRS * CPP_MODULES_PER_DIR;
    for _ in 0..8 {
        let module = random.below(cpp_modules);
        let (_, source_path) = cpp_module_path(module);
        let source = tree.read(&source_path);
        tree.write(
            &source_path,
            &format!("{source}int extra_{module}(int x) {{ return x; }}\n"),
        );
    }
    for _ in 0..4 {
        let module = random.below(RUST_MODULES_PER_CRATE);
        let path = format!("rust/rc1/src/m{module}.rs");
        let source = tree.read(&path);
        tree.write(
            &path,
            &format!("{source}pub fn extra_{module}() {{ f0(); }}\n"),
        );
    }
    for module in cpp_modules..cpp_modules + 5 {
        write_cpp_module(&tree, module, &mut random);
    }
    for module in [3, 77, 150] {
        let (header_path, source_path) = cpp_module_path(module);
        tree.remove(&header_path);
        tree.remove(&source_path);
    }

    let report = assert_incremental_equals_full(&tree, "synthetic-incremental-check");
    assert_eq!(report.deleted, 6);
    assert!(
        report.parsed < 40,
        "only edited and added files are parsed: {report:?}"
    );
}
