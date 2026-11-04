//! A program initializes perry-stdlib exactly when it links it.
//!
//! perry-stdlib reaches the link line two ways: feature detection over the
//! imports, and the provider pass over the HIR (a compiled program can call a
//! stdlib provider with no import that names it: `new SQL()` from "bun", an
//! Effect `Stream`). Both end in one answer, and the entry prologue's
//! `js_stdlib_init_dispatch` keys on that answer. A provider-only program
//! used to link stdlib without initializing it, so every stdlib dispatcher
//! stayed unregistered and `new SQL(":memory:")` threw "Invalid native
//! property owner". A program that links no stdlib emits no init.

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
    linked_stdlib: bool,
    run: Output,
}

fn build_and_run(source: &str) -> Built {
    ensure_runtime_archive();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), source).expect("write fixture");
    let binary = dir.path().join("fixture.bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg("main.ts")
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
    let log = String::from_utf8_lossy(&compile.stdout).into_owned()
        + &String::from_utf8_lossy(&compile.stderr);
    let run = Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    let ir = std::fs::read_to_string(dir.path().join(".perry-trace/llvm/main_ts.ll"))
        .or_else(|_| std::fs::read_to_string(dir.path().join(".perry-trace/llvm/main.ll")))
        .expect("read LLVM IR");
    Built {
        ir,
        linked_stdlib: log.contains("with stdlib"),
        run,
    }
}

fn calls_stdlib_init(ir: &str) -> bool {
    ir.lines().any(|line| {
        line.trim_start()
            .starts_with("call void @js_stdlib_init_dispatch()")
    })
}

#[test]
fn provider_only_program_initializes_the_stdlib_it_links() {
    // No import names perry-stdlib: only the provider pass sees
    // `js_bun_sql_new` and links stdlib for it.
    let built = build_and_run(
        "import { SQL } from \"bun\";\n\
         const sql = new SQL(\":memory:\");\n\
         console.log(\"made:\", typeof sql, typeof sql.begin);\n",
    );
    assert!(built.linked_stdlib, "the provider pass links perry-stdlib");
    assert!(
        calls_stdlib_init(&built.ir),
        "a program that links perry-stdlib calls js_stdlib_init_dispatch in its prologue"
    );
    assert_success("compiled fixture", &built.run);
    assert_eq!(
        String::from_utf8_lossy(&built.run.stdout),
        "made: function function\n"
    );
}

#[test]
fn program_without_stdlib_emits_no_stdlib_init() {
    let built = build_and_run("console.log(\"hello\");\n");
    assert!(!built.linked_stdlib, "hello links the runtime only");
    assert!(!calls_stdlib_init(&built.ir), "no stdlib, no stdlib init");
    assert_success("compiled fixture", &built.run);
    assert_eq!(String::from_utf8_lossy(&built.run.stdout), "hello\n");
}
