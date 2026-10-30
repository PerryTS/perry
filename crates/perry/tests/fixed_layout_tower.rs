//! The proven-receiver dispatch of a class whose layout the facts prove fixed
//! calls the proven-this body directly: the layout recheck (the keys/flags/
//! ShapeId diamond) has nothing left to catch. A class the facts only prove
//! stable keeps the recheck. Every program is compared with Node.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

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
        command.current_dir(workspace_root()).args([
            "build",
            "--release",
            "-p",
            "perry-runtime-static",
            "-p",
            "perry-stdlib-static",
        ]);
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build static runtime archives");
        assert!(build.status.success(), "static runtime build failed");
        assert!(
            Command::new("bash")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../scripts/gc_call_effects/sidecars.sh"
                ))
                .args(["--cargo-profile", "release"])
                .status()
                .is_ok_and(|status| status.success()),
            "scripts/gc_call_effects/sidecars.sh failed"
        );
    });
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Built {
    ir: String,
    perry_stdout: String,
    node_stdout: String,
}

fn build_and_run(file: &str) -> Built {
    ensure_runtime_archive();
    let dir = tempfile::tempdir().expect("tempdir");
    let source = workspace_root().join("test-files").join(file);
    let local = dir.path().join(file);
    std::fs::copy(&source, &local).expect("copy fixture");
    let binary = dir.path().join("fixture.bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(file)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_runtime_dir())
        .output()
        .expect("run perry compile");
    assert_success("perry compile", &compile);
    let run = Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert_success("compiled fixture", &run);
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(file)
        .output()
        .expect("run Node oracle");
    assert_success("Node oracle", &node);
    let stem = file.trim_end_matches(".ts").replace('.', "_");
    let ir = std::fs::read_to_string(dir.path().join(format!(".perry-trace/llvm/{stem}_ts.ll")))
        .or_else(|_| {
            std::fs::read_to_string(dir.path().join(format!(".perry-trace/llvm/{stem}.ll")))
        })
        .expect("read LLVM IR");
    Built {
        ir,
        perry_stdout: String::from_utf8(run.stdout).expect("utf8"),
        node_stdout: String::from_utf8(node.stdout).expect("utf8"),
    }
}

/// Blocks of the layout recheck the tower emits: one per case that kept it.
fn recheck_diamonds(ir: &str) -> usize {
    ir.lines()
        .filter(|line| line.starts_with("idispatch.case") && line.contains(".pshape"))
        .count()
}

/// Calls of a proven-this clone.
fn clone_calls(ir: &str) -> usize {
    ir.lines()
        .filter(|line| line.contains("call double @perry_method") && line.contains("$pshape"))
        .count()
}

#[test]
fn a_fixed_layout_drops_the_recheck_of_a_proven_dispatch() {
    let built = build_and_run("test_gap_cls5_fixed_layout_tower.ts");
    assert_eq!(built.perry_stdout, built.node_stdout);
    assert!(
        clone_calls(&built.ir) > 0,
        "the template method dispatches to the proven-this clones"
    );
    assert_eq!(
        recheck_diamonds(&built.ir),
        0,
        "the facts prove every class fixed: no case keeps the recheck"
    );
}

#[test]
fn a_stable_but_unfixed_layout_keeps_the_recheck() {
    let built = build_and_run("test_gap_cls5_fixed_layout_conditional.ts");
    assert_eq!(built.perry_stdout, built.node_stdout);
    assert!(
        recheck_diamonds(&built.ir) > 0,
        "a key assigned on some constructor paths is no fixed layout: the recheck stays"
    );
}

#[test]
fn layouts_changed_after_construction_stay_correct() {
    let built = build_and_run("test_gap_cls5_fixed_layout_mutation.ts");
    assert_eq!(built.perry_stdout, built.node_stdout);
}
