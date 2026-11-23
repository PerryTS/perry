//! A hoisted `let`/`const` of an async function stays the binding's
//! declaration. The state-machine transform used to rewrite the declarator
//! `const buf = new Float32Array(n)` into an assignment, so codegen counted
//! the binding as reassigned, lost its typed-array fact in every closure that
//! captures it, and each `buf[i] = v` in such a closure went through
//! `js_put_value_set_dynamic` (a Number key formatted to a String and parsed
//! back). The binding now keeps its declaration: a captured `const` or a
//! never-reassigned `let` gets the same typed store a sync body gets, and a
//! binding that is really reassigned to another kind keeps the dynamic store.
//! Every program is compared with Node.

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

/// Compile `source` with the LLVM trace, run it and Node on it, assert equal
/// output, and return the module IR.
fn ir_matching_node(source: &str) -> String {
    ensure_runtime_archive();
    let dir = tempfile::tempdir().expect("tempdir");
    let file = "main.ts";
    std::fs::write(dir.path().join(file), source).expect("write fixture");
    let binary = dir.path().join("fixture.bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .args(["compile", file, "-o"])
        .arg(&binary)
        .args(["--no-cache", "--trace", "llvm"])
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&node.stdout),
        "perry output differs from Node"
    );
    std::fs::read_to_string(dir.path().join(".perry-trace/llvm/main_ts.ll"))
        .or_else(|_| std::fs::read_to_string(dir.path().join(".perry-trace/llvm/main.ll")))
        .expect("read LLVM IR")
}

fn dynamic_stores(ir: &str) -> usize {
    ir.lines()
        .filter(|line| line.contains("call") && line.contains("@js_put_value_set_dynamic("))
        .count()
}

fn typed_stores(ir: &str) -> usize {
    ir.lines().filter(|line| line.contains("tav.set")).count()
}

const CONST_AFTER_AWAIT: &str = r#"
async function make(n: number) {
  await 0;
  const buf = new Float32Array(n);
  function fill(k: number) { for (let i = 0; i < k; i++) buf[i] = i * 0.5; return buf[1]; }
  return fill;
}
make(4).then((fill) => console.log(fill(4)));
"#;

#[test]
fn a_captured_const_of_an_async_fn_gets_the_typed_store() {
    let ir = ir_matching_node(CONST_AFTER_AWAIT);
    assert_eq!(
        dynamic_stores(&ir),
        0,
        "the const keeps its Float32Array fact"
    );
    assert!(typed_stores(&ir) > 0, "the store is the typed-array store");
}

#[test]
fn a_captured_let_never_reassigned_gets_the_typed_store() {
    let ir = ir_matching_node(&CONST_AFTER_AWAIT.replace("const buf", "let buf"));
    assert_eq!(
        dynamic_stores(&ir),
        0,
        "a let the body never writes is declared once"
    );
    assert!(typed_stores(&ir) > 0, "the store is the typed-array store");
}

#[test]
fn a_captured_let_reassigned_to_an_array_keeps_the_dynamic_store() {
    let ir = ir_matching_node(
        r#"
async function make(n: number) {
  let buf: any = new Float32Array(n);
  function fill(k: number) { for (let i = 0; i < k; i++) buf[i] = i * 0.5; return buf[1]; }
  fill(2);
  await 0;
  buf = [0, 0, 0];
  return fill;
}
make(4).then((fill) => console.log(fill(3), Array.isArray((fill as any).length)));
"#,
    );
    assert!(
        dynamic_stores(&ir) > 0,
        "a binding written with another kind is not proven a Float32Array"
    );
}
