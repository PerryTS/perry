//! #10821 / #340 / #341 — a timer handle must not be able to BE another
//! family's handle.
//!
//! Timer ids and the common native-registry ids (node:net, node:http, sqlite,
//! …) are two independent counters that both start at 1, and under the old
//! representation both travelled as small integers under `POINTER_TAG`. So the
//! first `setTimeout` of a program and the first `net.createServer()` of the
//! same program were **the same JS value**. Measured on v0.5.1618:
//!
//! ```text
//! 2 server === timer: true          (node: false)
//! 3 map size: 1 timer timer         (node: 2 server timer)
//! 4 TIMER FIRED                     — never printed: `srv.close()` cleared the timer
//! ```
//!
//! The third line is the damaging one. A type-erased `srv.close()` reached the
//! timer method arm in `native_call_method/primitive_methods.rs`, whose own
//! docstring named this hazard ("an HTTP/2 server handle 1 vs a `setTimeout`
//! id 1"), matched `close`, and cleared the timer — the server stayed open and
//! an unrelated timer silently never fired.
//!
//! With the timer family migrated to ordinary objects the timer is a pointer to
//! its own allocation, so it cannot equal a registry id, cannot share its Map
//! key, and cannot capture another family's method call.
//!
//! This test is the witness for that claim. It fails on the old representation
//! in three independent places, and none of them needs the two counters to be
//! deliberately aligned: they align on their own, on the first of each.

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
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
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
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The expected strings are node 26.8.1's output for the same program.
#[test]
fn a_timer_is_not_the_same_value_as_a_native_registry_handle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import * as net from "node:net";
const anyv = (x: any) => x;
// The first registered native handle and the first timer are both id 1.
const srv: any = net.createServer(() => {});
const t: any = setTimeout(() => { console.log("4 TIMER FIRED"); }, 60);
console.log("1 typeof:", typeof srv, typeof t);
console.log("2 server === timer:", anyv(srv) === anyv(t));
const m = new Map(); m.set(srv, "server"); m.set(t, "timer");
console.log("3 map size:", m.size, m.get(srv), m.get(t));
// Type-erased on purpose: this is the call that used to be swallowed by the
// timer arm and clear the timer instead of closing the server.
anyv(srv).close();
setTimeout(() => { console.log("5 done"); }, 200);
"#,
    );
    assert_eq!(
        stdout,
        "1 typeof: object object\n\
         2 server === timer: false\n\
         3 map size: 2 server timer\n\
         4 TIMER FIRED\n\
         5 done\n"
    );
}
