//! Guarded specialization of stable imported object-literal methods (#8775).

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

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn fixture_dir() -> PathBuf {
    workspace_root().join("test-files/fixtures/issue_8775_imported_object")
}

fn target_debug_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let output = command.output().expect("build static runtime archive");
        assert_success("static runtime build", &output);
    });
}

fn copy_fixture(dir: &Path) {
    for file in [
        "package.json",
        "adapter.js",
        "barrel.js",
        "runner.js",
        "main.js",
        "semantics.js",
    ] {
        std::fs::copy(fixture_dir().join(file), dir.join(file))
            .unwrap_or_else(|error| panic!("copy {file}: {error}"));
    }
}

fn compile(dir: &Path, entry: &str, explain: bool) -> PathBuf {
    ensure_runtime_archive();
    let binary = dir.join(format!("{entry}.bin"));
    let mut command = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")));
    command
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir());
    if explain {
        command.arg("--opt-report=json").arg("--explain-lowering");
    }
    remove_gc_env_overrides(&mut command);
    let output = command.output().expect("run Perry compile");
    assert_success("Perry compile", &output);
    binary
}

fn run(binary: &Path, dir: &Path, moving_gc: bool) -> String {
    let mut command = Command::new(binary);
    command.current_dir(dir);
    remove_gc_env_overrides(&mut command);
    if moving_gc {
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
        .expect("run Node oracle");
    assert_success("Node oracle", &output);
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
fn stable_imported_object_methods_use_guarded_direct_closure_bodies() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path());
    let binary = compile(temp.path(), "main.js", true);

    let node = run_node(temp.path(), "main.js");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);
    assert_eq!(node.trim(), r#"{"checksum":400000,"remaining":0}"#);

    let main_ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/main_js.ll"))
        .expect("read main LLVM IR");
    let adapter_ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/adapter_js.ll"))
        .expect("read adapter LLVM IR");
    let runner_ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/runner_js.ll"))
        .expect("read runner LLVM IR");
    for func_id in [6, 7, 8, 9] {
        let symbol = format!("perry_closure_adapter_js__{func_id}");
        assert!(
            adapter_ir.contains(&format!("define double @{symbol}(")),
            "producer closure must have external linkage: {symbol}"
        );
    }
    for func_id in [7, 8, 9] {
        let symbol = format!("perry_closure_adapter_js__{func_id}");
        let fast_block = runner_ir
            .split("\n\n")
            .find(|block| block.contains("object_method_cache.direct.") && block.contains(&symbol))
            .unwrap_or_else(|| panic!("no dynamic-object direct block for {symbol}:\n{runner_ir}"));
        assert!(
            !fast_block.contains("js_native_call_method_by_id")
                && !fast_block.contains("js_typed_feedback_native_call_method_by_id")
                && !fast_block.contains("js_native_call_value"),
            "direct block must not redispatch dynamically:\n{fast_block}"
        );
    }
    assert!(main_ir.contains("call double @js_native_call_method_by_id"));
    assert!(runner_ir.contains("call double @js_native_call_method_by_id"));
    assert!(main_ir.contains("call i32 @js_closure_exact_func_guard"));
    assert!(runner_ir.contains("call i32 @js_closure_exact_func_guard"));
    assert!(main_ir.contains("call i64 @js_object_own_method_cache_miss"));
    assert!(runner_ir.contains("call i64 @js_object_own_method_cache_miss"));
    assert!(!main_ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"));
    assert!(!runner_ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"));
    assert!(
        main_ir.contains("object_method_cache.direct.")
            && main_ir.contains("call double @__perry_wrap_perry_fn_runner_js__"),
        "the no-this concise wrapper method must be published and selected:\n{main_ir}"
    );
    assert!(
        main_ir
            .lines()
            .any(|line| line.starts_with("@perry_global_adapter_js__")
                && line.ends_with(" = external global double")),
        "consumer must load the producer's exported object identity:\n{main_ir}"
    );

    let records = read_native_records(temp.path());
    let selected: Vec<_> = records
        .iter()
        .filter(|record| record["consumer"] == "imported_object_literal_method_direct_call")
        .collect();
    assert!(
        !selected.is_empty(),
        "missing selected records: {records:#?}"
    );
    for record in selected {
        let notes = record_notes(record);
        assert!(notes.contains(&"receiver_provenance=imported_object_literal_metadata"));
        assert!(notes.contains(&"generic_dispatch_fallback=js_native_call_method_by_id"));
        assert!(notes
            .contains(&"guards=receiver_identity,live_shape_cache,own_key_slot,function_identity"));
        assert!(notes.contains(&"append_only_shape_successors=revalidated_and_cached"));
    }
    let dynamic: Vec<_> = records
        .iter()
        .filter(|record| record["consumer"] == "whole_program_object_literal_method_direct_call")
        .collect();
    assert!(
        dynamic.len() >= 4,
        "wrapper-parameter calls were not selected: {records:#?}"
    );
    for record in dynamic {
        let notes = record_notes(record);
        assert!(notes.contains(&"receiver_provenance=dynamic_local_with_producer_candidates"));
        assert!(notes.contains(&"generic_dispatch_fallback=js_native_call_method_by_id"));
        assert!(notes
            .contains(&"guards=receiver_identity,live_shape_cache,own_key_slot,function_identity"));
        assert!(notes.contains(&"append_only_shape_successors=revalidated_and_cached"));
    }
}

#[test]
fn mutations_rebinding_function_values_proxy_and_barrel_match_node() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path());
    let binary = compile(temp.path(), "semantics.js", false);
    let node = run_node(temp.path(), "semantics.js");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/semantics_js.ll"))
        .expect("read semantics LLVM IR");
    assert!(
        ir.contains("call double @perry_closure_adapter_js__7"),
        "barrel import must retain producer method provenance:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method_by_id"),
        "mutation-sensitive paths must retain the generic fallback:\n{ir}"
    );
}
