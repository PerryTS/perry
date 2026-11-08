//! A binding whose every write is `new K(..)` of one typed-array class `K`,
//! the first one an unconditional top-level statement of its body that no
//! reader can precede, holds a `K` at every read in every other body
//! (`collectors::definite_init`, the write-set rule). A closure that stores
//! into such a binding gets the typed store, and a closure that passes such a
//! `const` to a typed function calls its specialized `TaPtr` entry. A binding
//! written with two classes keeps the dynamic store. Every program is compared
//! with Node.

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

/// Calls of a specialized entry that takes a Float32Array pointer (`ta6`).
fn ta_ptr_calls(ir: &str) -> usize {
    ir.lines()
        .filter(|line| line.contains("call") && line.contains("$spec_ta6"))
        .count()
}

#[test]
fn a_late_initialized_captured_binding_gets_the_typed_store() {
    let ir = ir_matching_node(
        r#"
function make(n: number) {
  let buf: Float32Array;
  buf = new Float32Array(n);
  function fill(k: number): number { for (let i = 0; i < k; i++) buf[i] = i * 0.5; return buf[1]; }
  return fill;
}
console.log(make(8)(8));
"#,
    );
    assert_eq!(
        dynamic_stores(&ir),
        0,
        "every write is a Float32Array: the closure store is typed"
    );
    assert!(typed_stores(&ir) > 0, "the store is the typed-array store");
}

#[test]
fn same_class_writes_in_closures_keep_the_typed_store() {
    let ir = ir_matching_node(
        r#"
function run(): string {
  let b: Float32Array;
  b = new Float32Array(4);
  const grow = (n: number) => { b = new Float32Array(n); };
  const put = () => { b[1] = 300.3; b[2] = 2.5; return b[1] + "," + b[2] + "," + b.length; };
  const r1 = put();
  grow(8);
  return r1 + " " + put();
}
console.log(run());
"#,
    );
    assert_eq!(
        dynamic_stores(&ir),
        0,
        "a write set of one class is a proof"
    );
    assert!(typed_stores(&ir) > 0, "the store is the typed-array store");
}

#[test]
fn a_write_of_another_class_keeps_the_dynamic_store() {
    let ir = ir_matching_node(
        r#"
function run(): string {
  let b: Float32Array | Uint8Array;
  b = new Float32Array(4);
  function swap() { b = new Uint8Array(4); }
  function put(): string { for (let i = 0; i < 4; i++) b[i] = 300 + i; return b[1] + "," + b[2]; }
  const r1 = put();
  swap();
  return r1 + " " + put();
}
console.log(run());
"#,
    );
    assert!(
        dynamic_stores(&ir) > 0,
        "a binding written with two classes is not proven either"
    );
}

#[test]
fn a_closure_passes_a_proven_const_to_the_specialized_entry() {
    let ir = ir_matching_node(
        r#"
const NV = 64;
const sx = new Float32Array(NV), sy = new Float32Array(NV);
for (let i = 0; i < NV; i++) { sx[i] = i * 0.5; sy[i] = 1 / (i + 1); }
function acc(ax: Float32Array, ay: Float32Array, i0: number, r: number, g: number): number {
  let s = 0;
  for (let k = 0; k < 4; k++) s += ax[(i0 + k) & (NV - 1)] * r + ay[(i0 + k) & (NV - 1)] * g;
  return s;
}
const draw = () => { let s = 0; for (let t = 0; t < NV; t++) s += acc(sx, sy, t, 0.9, 0.8); return s; };
console.log(draw().toFixed(6), draw().toFixed(6));
"#,
    );
    assert!(
        ta_ptr_calls(&ir) > 0,
        "the arrow's call passes the module Float32Arrays as raw pointers"
    );
}
