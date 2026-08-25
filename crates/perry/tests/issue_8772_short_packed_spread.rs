//! End-to-end coverage for #8772: guarded direct method calls with a final
//! short packed-array spread tail.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GEN_GC_EVACUATE",
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
    workspace_root().join("test-files/fixtures/issue_8772_short_packed_spread")
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn target_debug_dir() -> PathBuf {
    if let Some(runtime) = std::env::var_os("PERRY_TEST_RUNTIME_DIR") {
        return PathBuf::from(runtime);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    target.join("debug")
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let runtime_dir = target_debug_dir();
        if runtime_dir.join("libperry_runtime.a").is_file()
            && runtime_dir.join("libperry_stdlib.a").is_file()
        {
            return;
        }
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        let output = command.output().expect("build static runtime archives");
        assert_success("static runtime build", &output);
    });
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_fixture(dir: &Path, file: &str) {
    std::fs::copy(fixture_dir().join(file), dir.join(file))
        .unwrap_or_else(|error| panic!("copy {file}: {error}"));
}

fn compile(dir: &Path, entry: &str) -> PathBuf {
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
        .arg("--opt-report=json")
        .arg("--explain-lowering")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir());
    remove_gc_env_overrides(&mut command);
    let compiled = command.output().expect("compile fixture");
    assert_success("perry compile", &compiled);
    output
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
    let output = run_node_output(dir, entry);
    assert_success("Node oracle", &output);
    String::from_utf8(output.stdout).expect("Node stdout is UTF-8")
}

fn run_node_output(dir: &Path, entry: &str) -> Output {
    Command::new("node")
        .current_dir(dir)
        .arg("--experimental-strip-types")
        .arg(entry)
        .output()
        .expect("run Node oracle")
}

fn run_failure(binary: &Path, dir: &Path, moving_gc: bool) -> Output {
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
    let output = command.output().expect("run failing compiled fixture");
    assert!(
        !output.status.success(),
        "compiled fixture unexpectedly passed"
    );
    output
}

fn read_lowering_artifacts(dir: &Path) -> String {
    let lowering = dir.join(".perry-trace/lowering");
    let run_dir = std::fs::read_dir(&lowering)
        .expect("read lowering directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("lowering run directory");
    std::fs::read_dir(run_dir)
        .expect("read lowering run")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("perry_native_reps_") && name.ends_with(".json")
            })
        })
        .map(|entry| std::fs::read_to_string(entry.path()).expect("read lowering artifact"))
        .collect::<String>()
}

fn function_ir<'a>(ir: &'a str, fragment: &str) -> &'a str {
    let name = ir
        .find(fragment)
        .unwrap_or_else(|| panic!("missing function {fragment:?}"));
    let start = ir[..name]
        .rfind("\ndefine ")
        .unwrap_or_else(|| panic!("missing definition for {fragment:?}"));
    let tail = &ir[start + 1..];
    let end = tail.find("\n}\n").expect("terminated function definition");
    &tail[..end + 2]
}

fn named_blocks(function: &str, prefixes: &[&str]) -> String {
    let mut selected = false;
    let mut result = String::new();
    for line in function.lines() {
        if !line.starts_with([' ', '\t']) && line.contains(':') {
            selected = prefixes.iter().any(|prefix| line.contains(prefix));
        }
        if selected {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[test]
fn repro_has_direct_empty_and_one_arms_and_matches_node_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path(), "main.ts");
    let binary = compile(temp.path(), "main.ts");

    let node = run_node(temp.path(), "main.ts");
    assert_eq!(node, "90000900000\n");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/main_ts.ll"))
        .expect("read main LLVM IR");
    let invoke = function_ir(&ir, "__invoke(");
    assert!(invoke.contains("call i32 @js_short_packed_spread_values("));
    assert!(invoke.contains("call i32 @js_method_direct_shape_class("));
    let direct = named_blocks(
        invoke,
        &[
            "short_spread.target0.arity0",
            "short_spread.target0.arity1",
            "short_spread.target1.arity0",
            "short_spread.target1.arity1",
        ],
    );
    assert!(direct.contains("call double @perry_method_") && direct.contains("__reset("));
    assert!(
        !direct.contains("js_native_call_method_apply")
            && !direct.contains("js_spread_tail_fallback_args")
    );
    assert!(invoke.contains("short_spread.fallback"));
    assert!(invoke.contains("js_spread_tail_fallback_args"));
    assert!(invoke.contains("js_native_call_method_apply_by_id"));

    let artifacts = read_lowering_artifacts(temp.path());
    for required in [
        "packed_spread_arities=0,1,2,3,4",
        "method=reset",
        "spread_guard=exact_ordinary_packed_array,no_holes,max_length_4",
        "method_identity_guard=js_method_direct_shape_class(class_id,shape_id,invalidation_slot)",
        "generic_fallback=js_spread_tail_fallback_args+js_native_call_method_apply_by_id",
    ] {
        assert!(
            artifacts.contains(required),
            "explain-lowering artifact must contain {required:?}\n{artifacts}"
        );
    }
}

#[test]
fn reverse_dependency_has_direct_arms_and_matches_node_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path(), "generic.ts");
    copy_fixture(temp.path(), "reverse.ts");
    let binary = compile(temp.path(), "reverse.ts");

    let node = run_node(temp.path(), "reverse.ts");
    assert_eq!(node, "90000900000\n");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/generic_ts.ll"))
        .expect("read generic consumer LLVM IR");
    let invoke = function_ir(&ir, "__invoke(");
    assert!(invoke.contains("call i32 @js_short_packed_spread_values("));
    assert!(invoke.contains("call i32 @js_method_direct_shape_class("));
    assert!(invoke.contains("@perry_method_reverse_ts__Position__reset("));
    assert!(invoke.contains("@perry_method_reverse_ts__Velocity__reset("));
    assert!(ir.contains("@perry_class_shape_id_reverse_ts__Position = external global i32"));
    assert!(ir.contains("@perry_class_shape_id_reverse_ts__Velocity = external global i32"));
    let direct = named_blocks(
        invoke,
        &[
            "short_spread.target0.arity0",
            "short_spread.target0.arity1",
            "short_spread.target1.arity0",
            "short_spread.target1.arity1",
        ],
    );
    assert!(!direct.contains("js_native_call_method_apply"));
    assert!(invoke.contains("short_spread.fallback"));
    assert!(invoke.contains("js_native_call_method_apply_by_id"));
}

#[test]
fn every_exotic_spread_and_dispatch_case_matches_node_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path(), "semantics.ts");
    let binary = compile(temp.path(), "semantics.ts");
    let node = run_node(temp.path(), "semantics.ts");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);
}

#[test]
fn throwing_iterator_matches_node_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_fixture(temp.path(), "throwing.ts");
    let binary = compile(temp.path(), "throwing.ts");

    let node = run_node_output(temp.path(), "throwing.ts");
    assert!(!node.status.success(), "Node fixture unexpectedly passed");
    assert!(String::from_utf8_lossy(&node.stderr).contains("iterator-boom"));

    for moving_gc in [false, true] {
        let perry = run_failure(&binary, temp.path(), moving_gc);
        let stderr = String::from_utf8_lossy(&perry.stderr);
        assert!(
            stderr.contains("iterator-boom"),
            "Perry error must preserve iterator throw, got:\n{stderr}"
        );
    }
}

#[test]
fn mixed_math_fixed_prefix_and_spread_tail_matches_node_under_moving_gc() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("math.ts"),
        r#"
const rows = [[], [0], [0, 1], [2, 3], [3, 4, 5]];
for (const values of rows) {
  console.log(JSON.stringify({
    values,
    max: Math.max(-1, ...values),
    min: Math.min(99, ...values),
  }));
}
"#,
    )
    .expect("write mixed Math spread fixture");
    let binary = compile(temp.path(), "math.ts");
    let node = run_node(temp.path(), "math.ts");
    assert_eq!(run(&binary, temp.path(), false), node);
    assert_eq!(run(&binary, temp.path(), true), node);

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/math_ts.ll"))
        .expect("read mixed Math spread LLVM IR");
    assert!(
        ir.contains("js_native_call_method_apply_by_id"),
        "mixed fixed/spread Math calls must retain iterator-aware apply\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_math_max2") && !ir.contains("call double @js_math_min2"),
        "the spread array must not be coerced as one scalar operand\n{ir}"
    );
}
