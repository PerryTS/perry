//! Regression: a native builtin (`JSON.parse` / `JSON.stringify`) used as the
//! result of a `||` / `??` / `&&` short-circuit expression that is then CALLED
//! must materialize the real callable, not `undefined`.
//!
//! Before the fix, `(K || JSON.parse)('...')` threw `TypeError: value is not a
//! function`. Root cause was in perry-hir: lowering a call sets the
//! `lowering_call_callee` marker before lowering the callee, but a logical
//! expression in callee position (`(K || JSON.parse)`) is itself the callee —
//! its operands are values, not the immediate callee member. The marker leaked
//! through `lower_bin_expr` into the `JSON.parse` operand, so the member-tail
//! reroute-undo collapsed it to the value-less intrinsic form
//! `PropertyGet { GlobalGet(0), "parse" }` (the namespace name dropped), which
//! lowers to `undefined`. Stored-then-called (`let g = K || JSON.parse; g(x)`)
//! and direct calls (`JSON.parse(x)`) were unaffected because the marker was
//! correctly false / true there.
//!
//! Fix: `lower_bin_expr` clears `lowering_call_callee` while lowering its
//! operands, so a nested builtin-namespace member keeps its reified value form.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // Wall-clock timeout: kill + fail fast rather than stalling CI if the
    // compiled binary misbehaves. Output is a few short lines, so the pipes
    // can't fill before exit.
    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");
    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait on compiled binary") {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("compiled binary did not exit within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut stdout).ok();
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_string(&mut stderr).ok();
    }
    assert!(
        status.success(),
        "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn short_circuit_native_builtin_callee() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// Native builtin reached via || in callee position, then called.
function viaOr(K: any): any { return (K || JSON.parse)('{"x":1}'); }
// Native builtin via ?? in callee position.
function viaCoalesce(K: any): any { return (K ?? JSON.parse)('{"x":2}'); }
// JSON.stringify via || in callee position.
function stringifyViaOr(K: any): string { return (K || JSON.stringify)({ y: 3 }); }
// Native builtin via && in callee position (K truthy -> returns JSON.parse).
function viaAnd(K: any): any { return (K && JSON.parse)('{"x":6}'); }
// USER function via || must keep working.
function userViaOr(K: any): any { const u = (s: string) => JSON.parse(s); return (K || u)('{"x":4}'); }
// Stored-then-called must keep working.
function stored(K: any): any { let g = K || JSON.parse; return g('{"x":7}'); }
// Direct intrinsic call must keep its fast path.
function direct(s: string): any { return JSON.parse(s); }
// Native builtin via ternary in callee position.
function viaTernary(K: any): any { return (K ? K : JSON.parse)('{"x":8}'); }
// Native builtin via comma sequence in callee position.
function viaComma(K: any): any { return (0, JSON.parse)('{"x":9}'); }

console.log("OR:" + viaOr(undefined).x);
console.log("COALESCE:" + viaCoalesce(undefined).x);
console.log("STRINGIFY:" + stringifyViaOr(undefined));
console.log("AND:" + viaAnd("truthy").x);
console.log("USER:" + userViaOr(undefined).x);
console.log("STORED:" + stored(undefined).x);
console.log("DIRECT:" + direct('{"x":5}').x);
console.log("TERNARY:" + viaTernary(undefined).x);
console.log("COMMA:" + viaComma(undefined).x);
"#,
    );

    assert!(out.contains("OR:1"), "JSON.parse via || lost: {out}");
    assert!(out.contains("COALESCE:2"), "JSON.parse via ?? lost: {out}");
    assert!(
        out.contains("STRINGIFY:{\"y\":3}"),
        "JSON.stringify via || lost: {out}"
    );
    assert!(out.contains("AND:6"), "JSON.parse via && lost: {out}");
    assert!(out.contains("USER:4"), "user fn via || regressed: {out}");
    assert!(
        out.contains("STORED:7"),
        "stored-then-called regressed: {out}"
    );
    assert!(
        out.contains("DIRECT:5"),
        "direct JSON.parse call regressed: {out}"
    );
    assert!(
        out.contains("TERNARY:8"),
        "JSON.parse via ternary lost: {out}"
    );
    assert!(out.contains("COMMA:9"), "JSON.parse via comma lost: {out}");
}
