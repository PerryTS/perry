//! A call site the whole-program facts close over its receiver classes calls
//! exactly the bodies those classes resolve to; such a body that fits the
//! guarded-specialization budget joins the early inliner (`alwaysinline` in
//! the pre-optimization IR), so the call boundary disappears. Nothing is
//! admitted without the proof: a method reachable only through a receiver
//! the facts cannot close, in a program that rewrites the prototype, keeps
//! its call boundary. Both programs are compared with Node for semantics.

use std::path::{Path, PathBuf};
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

/// The definition line of the module's public body of `Class.method`.
fn definition<'a>(ir: &'a str, class: &str, method: &str) -> &'a str {
    let needle = format!("__{class}__{method}(double %this_arg");
    ir.lines()
        .find(|line| line.starts_with("define ") && line.contains(&needle))
        .unwrap_or_else(|| panic!("no definition of {class}.{method} in the IR"))
}

#[test]
fn facts_closed_targets_enter_the_early_inliner() {
    let built = build_and_run("test_gap_cls5_closed_target_inline.ts");
    assert_eq!(built.perry_stdout, built.node_stdout);
    for method in ["describe", "scaled"] {
        let line = definition(&built.ir, "Shape", method);
        assert!(
            line.contains(" alwaysinline "),
            "Shape.{method} is the facts-closed target of a proven site and must join the early inliner:\n{line}"
        );
    }
    // Overrides the loops never reach stay out of line.
    for (class, method) in [("Circle", "area"), ("Rect", "area")] {
        let line = definition(&built.ir, class, method);
        assert!(
            !line.contains(" alwaysinline "),
            "{class}.{method} is no closed target of a direct site:\n{line}"
        );
    }
}

#[test]
fn an_unproven_site_admits_nothing_and_mutation_stays_correct() {
    let built = build_and_run("test_gap_cls5_closed_target_mutation.ts");
    assert_eq!(built.perry_stdout, built.node_stdout);
    for (class, method) in [("Base", "show"), ("Shape", "describe")] {
        let line = definition(&built.ir, class, method);
        assert!(
            !line.contains(" alwaysinline "),
            "{class}.{method}: no proof, no early inline:\n{line}"
        );
    }
}
