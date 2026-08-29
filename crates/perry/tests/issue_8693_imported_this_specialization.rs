//! Cross-module proven-`this` specialization (#8693).
//!
//! The producer is the authority for clone eligibility. Importers may call the
//! published clone only behind the ordinary exact-class/shape guard; its
//! runtime-dispatch fallback remains present for every mutation-sensitive
//! semantic case.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

/// Perry ships a `panic=abort` runtime. A debug `panic=unwind` archive plants
/// abort-on-unwind guards in `extern "C"` helpers, so the raw JS exception in
/// the extracted-method case cannot reach the generated catch landing pad.
fn target_runtime_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("release")
    } else {
        target.join("release")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            // `panic` is profile-level; this must match `target_runtime_dir()`
            // and the runtime Perry actually ships.
            .arg("--release")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build static runtime archives");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn fixture_dir() -> PathBuf {
    workspace_root().join("test-files/fixtures/issue_8693_imported_this")
}

fn copy_fixture(dir: &Path) {
    for file in [
        "package.json",
        "registry.js",
        "barrel.js",
        "main.js",
        "semantics.js",
    ] {
        std::fs::copy(fixture_dir().join(file), dir.join(file))
            .unwrap_or_else(|error| panic!("copy {file}: {error}"));
    }
}

fn compile(dir: &Path, entry: &str, explain: bool) -> PathBuf {
    ensure_runtime_archive();
    let output = dir.join(format!("{entry}.bin"));
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_runtime_dir());
    if explain {
        command.arg("--opt-report=json").arg("--explain-lowering");
    }
    remove_gc_env_overrides(&mut command);
    let result = command.output().expect("run perry compile");
    assert_success("perry compile", &result);
    output
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(binary: &Path, dir: &Path, force_evacuation: bool) -> String {
    let mut command = Command::new(binary);
    command.current_dir(dir);
    remove_gc_env_overrides(&mut command);
    if force_evacuation {
        command
            .env("PERRY_GC_SCAVENGE", "1")
            .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_INCREMENTAL", "0");
    }
    let output = command.output().expect("run compiled fixture");
    assert_success("compiled fixture", &output);
    String::from_utf8(output.stdout).expect("fixture stdout is UTF-8")
}

fn run_node(dir: &Path, entry: &str) -> String {
    let output = Command::new("node")
        .current_dir(dir)
        .arg(entry)
        .output()
        .expect("run Node semantic oracle");
    assert_success("Node semantic oracle", &output);
    String::from_utf8(output.stdout).expect("Node stdout is UTF-8")
}

fn read_native_records(dir: &Path) -> Vec<serde_json::Value> {
    let lowering = dir.join(".perry-trace/lowering");
    let run_dir = std::fs::read_dir(&lowering)
        .expect("read lowering directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("lowering run directory");
    let mut records = Vec::new();
    for entry in std::fs::read_dir(run_dir).expect("read lowering run") {
        let path = entry.expect("lowering entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("perry_native_reps_") || !name.ends_with(".json") {
            continue;
        }
        let artifact: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        records.extend(
            artifact["records"]
                .as_array()
                .unwrap_or_else(|| panic!("missing records in {}", path.display()))
                .iter()
                .cloned(),
        );
    }
    records
}

fn record_notes(record: &serde_json::Value) -> Vec<&str> {
    record["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

#[test]
fn imported_registry_uses_published_proven_this_clones_with_fallbacks() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path());
    let binary = compile(temp.path(), "main.js", true);

    let perry_output = run(&binary, temp.path(), false);
    let node_output = run_node(temp.path(), "main.js");
    let perry: serde_json::Value = serde_json::from_str(perry_output.trim()).expect("Perry JSON");
    let node: serde_json::Value = serde_json::from_str(node_output.trim()).expect("Node JSON");
    assert_eq!(perry["remaining"], 0);
    assert_eq!(perry["remaining"], node["remaining"]);

    let main_ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/main_js.ll"))
        .expect("read main LLVM IR");
    let registry_ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/registry_js.ll"))
        .expect("read registry LLVM IR");
    for method in ["add", "remove"] {
        let clone = format!("perry_method_registry_js__Registry__{method}$pshape");
        assert!(
            main_ir.contains(&format!("declare double @{clone}(")),
            "importer must declare producer clone {clone}:\n{main_ir}"
        );
        let fast_block = main_ir
            .split("\n\n")
            .find(|block| block.contains("method_direct.fast.") && block.contains(&clone))
            .unwrap_or_else(|| panic!("no direct fast block calling {clone}:\n{main_ir}"));
        assert!(
            !fast_block.contains("js_native_call_method_by_id")
                && !fast_block.contains("js_typed_feedback_native_call_method_by_id"),
            "stable fast arm must not use generic method dispatch:\n{fast_block}"
        );
    }
    for method in ["pushEntity", "removeEntity"] {
        let clone = format!("perry_method_registry_js__Group__{method}$pshape");
        assert!(
            registry_ir.contains(&format!("define double @{clone}(")),
            "producer clone must have external linkage: {clone}\n{registry_ir}"
        );
        let definition = registry_ir
            .lines()
            .find(|line| line.contains(&format!("define double @{clone}(")))
            .unwrap_or_else(|| panic!("missing clone definition for {clone}:\n{registry_ir}"));
        assert!(
            definition.contains("inlinehint") || definition.contains("alwaysinline"),
            "small producer clone must be admitted to inlining: {definition}"
        );
        assert!(
            registry_ir.contains(&format!("call double @{clone}(")),
            "Registry clone must directly select Group clone: {clone}\n{registry_ir}"
        );
    }
    assert!(
        main_ir.contains("call double @js_native_call_method_by_id"),
        "guard failure must retain generic runtime fallback:\n{main_ir}"
    );

    let records = read_native_records(temp.path());
    for method in ["add", "remove"] {
        let suffix = format!("Registry__{method}$pshape");
        let selected = records.iter().find(|record| {
            let notes = record_notes(record);
            record["consumer"] == "proven_this_method_direct_call"
                && notes
                    .iter()
                    .any(|note| note.starts_with("typed_clone=") && note.ends_with(&suffix))
        });
        let record = selected.unwrap_or_else(|| {
            panic!("no lowering selection for imported {method}:\n{records:#?}")
        });
        let notes = record_notes(record);
        assert!(notes.contains(&"receiver_provenance=imported_class_metadata"));
        assert!(notes.contains(&"this_representation=tagged_js_value_exact_shape"));
        assert!(notes.contains(&"generic_dispatch_fallback=js_native_call_method_by_id"));
        assert!(
            !notes
                .iter()
                .any(|note| *note == "typed_clone_rejected=captures_this"),
            "valid imported method must not be rejected for capturing this: {record:#}"
        );
    }
}

#[test]
fn imported_clone_guards_preserve_all_method_semantics_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path());
    let binary = compile(temp.path(), "semantics.js", false);
    let node = run_node(temp.path(), "semantics.js");
    let ordinary = run(&binary, temp.path(), false);
    let moving = run(&binary, temp.path(), true);
    assert_eq!(ordinary, node, "ordinary Perry output differs from Node");
    assert_eq!(
        moving, node,
        "forced-moving-GC Perry output differs from Node"
    );

    let semantics_ir =
        std::fs::read_to_string(temp.path().join(".perry-trace/llvm/semantics_js.ll"))
            .expect("read semantic fixture LLVM IR");
    let clone = "perry_method_barrel_js__TowerRegistry__cycle$pshape";
    assert!(
        semantics_ir.split("\n\n").any(|block| {
            block.contains("idispatch.case") && block.contains(&format!("call double @{clone}("))
        }),
        "stable adapter-field tower must route to imported clone {clone}:\n{semantics_ir}"
    );
}
