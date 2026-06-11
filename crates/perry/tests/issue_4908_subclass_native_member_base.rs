//! Regression test for #4908: subclassing a native member base whose trailing
//! property name equals the subclass's OWN name — the canonical node:http
//! shape `class Agent extends http.Agent { ... }` — OOM-crashed codegen.
//!
//! Root cause: the class-heritage lowering resolves a member-expression base
//! (`http.Agent`) by its trailing property only (`extract_member_class_name`
//! returns "Agent"). The subclass registers its own name *before* heritage
//! resolution, so `lookup_class("Agent")` returned the class itself and both
//! `extends` and `extends_name` self-referenced. Every codegen parent-chain
//! walk then looped forever, killing the process during "Generating code...".
//!
//! Four node:http tests (`test-http-client-abort-keep-alive-destroy-res`,
//! `-abort-keep-alive-queued-tcp-socket`, `-abort-unix-socket`,
//! `-read-in-error`) all share this `class Agent extends http.Agent` shape.
//!
//! Fix: when the member base's trailing name equals the subclass name, leave
//! the class parentless (no self-link), matching how a non-colliding native
//! member base already behaves. These cases must now compile AND construct
//! without crashing.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
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
        "perry compile failed (pre-fix: codegen OOM-loop on self-referential \
         extends)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The exact failing shape: `class Agent extends http.Agent` (name collision
/// with the `Agent` property), an overriding method that calls
/// `super.createConnection(...)`, and a captured module-level `let` mutated via
/// `++`. Pre-fix this OOM-looped codegen; it must now compile and construct.
#[test]
fn subclass_http_agent_with_super_and_captured_let_compiles_and_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
let socketsCreated = 0;

class Agent extends http.Agent {
  createConnection(options: any, oncreate: any) {
    socketsCreated++;
    return super.createConnection(options, oncreate);
  }
}

const a = new Agent();
console.log('ctor-ok', typeof a === 'object');
console.log('method-ok', typeof a.createConnection === 'function');
console.log('captured-let', socketsCreated);
"#,
    );
    assert_eq!(
        stdout, "ctor-ok true\nmethod-ok true\ncaptured-let 0\n",
        "`class Agent extends http.Agent` must compile and construct cleanly"
    );
}

/// The minimal trigger with no method, no super, no captured let: an *empty*
/// `class Agent extends http.Agent {}` was already enough to OOM-crash codegen
/// (the self-referential parent edge, not the method body, is the loop). Guards
/// against a narrower fix that only handles the method-bearing shape.
#[test]
fn empty_subclass_of_self_named_member_base_compiles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
class Agent extends http.Agent {}
console.log('ok', typeof new Agent() === 'object');
"#,
    );
    assert_eq!(
        stdout, "ok true\n",
        "empty self-named subclass must compile"
    );
}
