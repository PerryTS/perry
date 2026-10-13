//! Contract emission must participate in the same per-module cache identity
//! as the ordinary lowering options, in both directions.
#![cfg(target_os = "linux")]

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(project: &Path, contracts: bool) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    cmd.current_dir(project)
        .args(["--format", "json", "compile", "main.ts", "-o", "app"])
        .arg("--cache-dir")
        .arg(project.join("cache"))
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_DISABLE_BUILD_CACHE")
        .env_remove("PERRY_CHECK_CONTRACTS")
        .env_remove("PERRY_METHOD_CERT_SABOTAGE")
        .env_remove("PERRY_B4_SABOTAGE")
        .env_remove("PERRY_ASSERT_CAPTURE_CELLS")
        .env_remove("PERRY_OBJECT_CACHE_BUILD_ID");
    if contracts {
        cmd.env("PERRY_CHECK_CONTRACTS", "1");
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .expect("compile JSON");
    serde_json::from_str(line).expect("parse compile JSON")
}

fn objects(cache: &Path) -> BTreeSet<PathBuf> {
    let mut result = BTreeSet::new();
    for target in fs::read_dir(cache.join("objects")).expect("object cache") {
        for entry in fs::read_dir(target.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "o") {
                result.insert(path);
            }
        }
    }
    result
}

fn contract_references(object: &Path) -> Vec<String> {
    // The linked binary can be stripped and the runtime archive can contain
    // unused contract definitions. Inspect references in the object actually
    // supplied to the linker, matching every contract entry rather than a
    // fixed symbol list.
    let out = Command::new("nm")
        .arg("--undefined-only")
        .arg(object)
        .output()
        .expect("nm (binutils) is required");
    assert!(out.status.success(), "nm: {:?}", out);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter(|name| name.ends_with("_contract_check") || name.ends_with("_contract"))
        .map(str::to_owned)
        .collect()
}

fn assert_output(project: &Path) {
    let out = Command::new(project.join("app"))
        .env_remove("PERRY_CHECK_CONTRACTS")
        .output()
        .expect("run fixture");
    assert!(out.status.success(), "fixture failed: {:?}", out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true 1 2\n");
}

#[test]
fn contract_then_release_objects_are_isolated_in_shared_cache() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    fs::write(
        project.join("package.json"),
        "{\"name\":\"contract-cache-test\"}\n",
    )
    .unwrap();
    fs::write(
        project.join("main.ts"),
        r#"const a=[1]; a.push(2);
// These simple methods can prove their contracts in the callee. Keep a
// dynamically typed Date call to exercise a pristine direct arm at the site.
const d: any = process.argv.length > 0 ? new Date(0) : {};
if (d.getTime() !== 0) throw new Error("Date guard failed");
console.log(/b/.test("abc"), Object.keys({x:1}).length, a.length);
"#,
    )
    .unwrap();
    let cache = project.join("cache");

    let contracts = compile(project, true);
    assert_eq!(contracts["codegen_cache"]["misses"], 1, "{contracts}");
    let contract_objects = objects(&cache);
    assert_eq!(contract_objects.len(), 1, "{contract_objects:?}");
    let contract_object = contract_objects.first().unwrap();
    let references = contract_references(contract_object);
    assert!(
        references
            .iter()
            .any(|name| name == "js_builtin_proto_contract_check"),
        "fixture must exercise a pristine direct builtin arm: {references:?}"
    );
    assert_output(project);

    let release = compile(project, false);
    // Exercise the stripped-binary case explicitly: its linked object's
    // undefined-symbol list remains the evidence for emitted references.
    let stripped = Command::new("strip")
        .arg(project.join("app"))
        .output()
        .expect("strip (binutils) is required");
    assert!(stripped.status.success(), "strip failed: {stripped:?}");
    let all_objects = objects(&cache);
    let fresh: Vec<_> = all_objects.difference(&contract_objects).collect();
    assert!(fresh.len() <= 1, "unexpected objects: {fresh:?}");
    // If the key change is removed, the release compile reuses this contract
    // object. Check its symbols before cache statistics so the negative
    // control fails on the stale contract references themselves.
    let release_object = fresh.first().copied().unwrap_or(contract_object);
    assert_eq!(
        contract_references(release_object),
        Vec::<String>::new(),
        "release object's contract references must be zero"
    );
    assert_eq!(release["build_cache"]["hit"], false, "{release}");
    assert_eq!(release["codegen_cache"]["hits"], 0, "{release}");
    assert_eq!(release["codegen_cache"]["misses"], 1, "{release}");
    assert_eq!(fresh.len(), 1);
    assert_output(project);

    // A release-first cache must also miss when contracts are requested.
    let reverse = tempfile::tempdir().unwrap();
    let reverse_project = reverse.path();
    for name in ["main.ts", "package.json"] {
        fs::copy(project.join(name), reverse_project.join(name)).unwrap();
    }
    let cold_release = compile(reverse_project, false);
    assert_eq!(cold_release["codegen_cache"]["misses"], 1, "{cold_release}");
    let release_objects = objects(&reverse_project.join("cache"));
    assert_eq!(release_objects.len(), 1);
    assert!(contract_references(release_objects.first().unwrap()).is_empty());
    let cold_contracts = compile(reverse_project, true);
    assert_eq!(
        cold_contracts["codegen_cache"]["hits"], 0,
        "{cold_contracts}"
    );
    assert_eq!(
        cold_contracts["codegen_cache"]["misses"], 1,
        "{cold_contracts}"
    );
    let reverse_objects = objects(&reverse_project.join("cache"));
    let fresh_contracts: Vec<_> = reverse_objects.difference(&release_objects).collect();
    assert_eq!(fresh_contracts.len(), 1);
    assert!(!contract_references(fresh_contracts[0]).is_empty());
    assert_output(reverse_project);

    // Existing objects remain reusable when switching back to their mode.
    let contracts_again = compile(project, true);
    assert_eq!(
        contracts_again["codegen_cache"]["hits"], 1,
        "{contracts_again}"
    );
    let release_again = compile(project, false);
    assert_eq!(release_again["codegen_cache"]["hits"], 1, "{release_again}");
    assert_output(project);
}
