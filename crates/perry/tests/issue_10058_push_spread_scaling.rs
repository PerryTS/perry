//! Regression coverage for #10058: repeated fixed-size spread pushes must do
//! work proportional to the appended tail while preserving iterator and GC
//! semantics.

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

fn fixture() -> PathBuf {
    workspace_root().join("test-files/fixtures/issue_10058_push_spread/main.ts")
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

fn compile(dir: &Path) -> (PathBuf, String) {
    ensure_runtime_archive();
    let output = dir.join("main.bin");
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(fixture())
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir());
    remove_gc_env_overrides(&mut command);
    let compiled = command.output().expect("compile fixture");
    assert_success("perry compile", &compiled);
    (
        output,
        String::from_utf8_lossy(&compiled.stderr).into_owned(),
    )
}

fn run(binary: &Path, moving_gc: bool) -> Output {
    let mut command = Command::new(binary);
    remove_gc_env_overrides(&mut command);
    if moving_gc {
        command
            .env("PERRY_GC_SCAVENGE", "1")
            .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_INCREMENTAL", "0");
    }
    command.output().expect("run compiled fixture")
}

fn run_node() -> Output {
    Command::new("node")
        .arg("--expose-gc")
        .arg("--experimental-strip-types")
        .arg(fixture())
        .output()
        .expect("run Node oracle")
}

#[test]
fn spread_push_is_iterator_correct_and_gc_safe_on_reused_destinations() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (binary, compile_stderr) = compile(temp.path());
    let ir_path = compile_stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{compile_stderr}")
        });
    let ir = std::fs::read_to_string(ir_path).expect("read kept LLVM IR");
    assert!(
        ir.contains("call i64 @js_array_spread_append("),
        "typed-local spread push must retain the iterator-aware runtime entry\n{ir}"
    );

    let node = run_node();
    assert_success("Node oracle", &node);
    for moving_gc in [false, true] {
        let perry = run(&binary, moving_gc);
        assert_success("compiled fixture", &perry);
        assert_eq!(
            perry.stdout,
            node.stdout,
            "spread-push parity failed with moving_gc={moving_gc}\nPerry stderr:\n{}",
            String::from_utf8_lossy(&perry.stderr)
        );
    }
}
