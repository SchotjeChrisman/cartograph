//! Reads the manifests that change how source files are understood: Cargo packages and
//! `compile_commands.json`.

use std::path::Path;

use languages::{CompileCommands, ProjectContext, RustCrate, parent_dir};
use serde::Deserialize;

use crate::walk::Scan;

/// Where a compile database is looked for, relative to the root. Build directories are usually
/// gitignored, so the walk would not find them.
/// The Composer package whose presence in a manifest's requirements marks a Laravel project.
const LARAVEL_PACKAGE: &str = "laravel/framework";

const COMPILE_COMMANDS_LOCATIONS: [&str; 2] =
    ["compile_commands.json", "build/compile_commands.json"];

#[derive(Deserialize)]
struct Manifest {
    package: Option<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
}

#[derive(Deserialize, Default)]
struct ComposerManifest {
    #[serde(default)]
    require: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "require-dev")]
    require_dev: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The context plus a warning for each manifest that could not be understood.
pub fn read_context(root: &Path, scan: &Scan) -> (ProjectContext, Vec<String>) {
    let mut warnings = Vec::new();
    let mut rust_crates = Vec::new();
    for manifest in &scan.cargo_manifests {
        match read_package_name(&root.join(manifest)) {
            Ok(Some(name)) => rust_crates.push(RustCrate {
                dir: parent_dir(manifest).to_string(),
                name: name.replace('-', "_"),
            }),
            Ok(None) => {}
            Err(message) => warnings.push(format!("{manifest}: {message}")),
        }
    }
    let compile_commands = read_compile_commands(root, &mut warnings);
    let laravel = scan
        .composer_manifests
        .iter()
        .any(|manifest| requires_laravel(root, manifest, &mut warnings));
    (
        ProjectContext {
            rust_crates,
            compile_commands,
            laravel,
        },
        warnings,
    )
}

fn requires_laravel(root: &Path, manifest: &str, warnings: &mut Vec<String>) -> bool {
    let parsed = std::fs::read_to_string(root.join(manifest))
        .map_err(|error| error.to_string())
        .and_then(|text| {
            serde_json::from_str::<ComposerManifest>(&text).map_err(|error| error.to_string())
        });
    match parsed {
        Ok(composer) => {
            composer.require.contains_key(LARAVEL_PACKAGE)
                || composer.require_dev.contains_key(LARAVEL_PACKAGE)
        }
        Err(message) => {
            warnings.push(format!("{manifest}: {message}"));
            false
        }
    }
}

fn read_package_name(path: &Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let manifest: Manifest = toml::from_str(&text).map_err(|error| error.to_string())?;
    Ok(manifest.package.map(|package| package.name))
}

fn read_compile_commands(root: &Path, warnings: &mut Vec<String>) -> CompileCommands {
    for location in COMPILE_COMMANDS_LOCATIONS {
        let path = root.join(location);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match CompileCommands::parse(&text, root) {
            Ok(commands) => return commands,
            Err(error) => warnings.push(format!("{location}: {error}")),
        }
    }
    CompileCommands::default()
}
