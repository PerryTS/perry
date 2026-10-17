//! The runtime build id (`perry-runtime/build.rs`, `RUNTIME_BUILD_INPUTS`) is
//! a hash of the source trees the archive is built from. A local crate the
//! runtime builds with but the list leaves out would let an archive go stale
//! under an unchanged id; a compiler-only crate in the list rebuilds and
//! re-stamps identical archives on every codegen edit. This checks the list
//! against the runtime's local dependency closure from `cargo metadata`.
//!
//! Sabotage check: drop `crates/perry-hex/src` from the list, or add
//! `crates/perry-hir/src` to it, and this fails.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

const BUILD_RS: &str = include_str!("../../perry-runtime/build.rs");

fn listed_inputs() -> BTreeSet<String> {
    let start = BUILD_RS
        .find("const RUNTIME_BUILD_INPUTS: &[&str] = &[")
        .expect("RUNTIME_BUILD_INPUTS in perry-runtime/build.rs");
    let body = &BUILD_RS[start..];
    let body = &body[body.find('[').unwrap() + 1..];
    let body = &body[body.find('[').unwrap() + 1..body.find("];").unwrap()];
    body.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn runtime_closure(root: &Path) -> BTreeSet<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages: HashMap<&str, &serde_json::Value> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| (package["name"].as_str().unwrap(), package))
        .collect();
    let mut closure = BTreeSet::new();
    let mut pending = vec!["perry-runtime"];
    while let Some(name) = pending.pop() {
        let package = packages[name];
        let manifest = PathBuf::from(package["manifest_path"].as_str().unwrap());
        let dir = manifest.parent().unwrap().strip_prefix(root).unwrap();
        if !closure.insert(dir.to_string_lossy().replace('\\', "/")) {
            continue;
        }
        for dependency in package["dependencies"].as_array().unwrap() {
            if dependency["path"].is_null() || dependency["kind"].as_str() == Some("dev") {
                continue;
            }
            pending.push(dependency["name"].as_str().unwrap());
        }
    }
    closure
}

#[test]
fn runtime_build_inputs_cover_the_runtime_closure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut expected: BTreeSet<String> = ["Cargo.toml", "Cargo.lock"].map(String::from).into();
    for dir in runtime_closure(&root) {
        expected.insert(format!("{dir}/Cargo.toml"));
        expected.insert(format!("{dir}/src"));
        if root.join(&dir).join("build.rs").is_file() {
            expected.insert(format!("{dir}/build.rs"));
        }
    }
    assert_eq!(
        listed_inputs(),
        expected,
        "RUNTIME_BUILD_INPUTS in crates/perry-runtime/build.rs must name exactly the runtime's local dependency closure"
    );
}
