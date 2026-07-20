//! E2E for #6563: runtime-native pty under the node-pty JS shape.
//!
//! Three compiled-TS scenarios, mirroring what the two target apps do:
//!
//! 1. `import { spawn } from "node-pty"` — spawn `sh` inside a real pty,
//!    write `echo hello`, assert the output arrives via `onData`, resize,
//!    then `exit` and assert `onExit` fires with `exitCode: 0`.
//! 2. `import * as pty from "@lydell/node-pty"` (opencode's package name) —
//!    `kill("SIGTERM")` must surface as `onExit { signal: 15 }`.
//! 3. dynamic `await import("node-pty")` (kimi-code's load path) — the
//!    namespace's `spawn` must be live and round-trip data.
//!
//! Headless-safe by construction: the pty pair is freshly allocated
//! (`openpty`), so no controlling terminal on the test process is needed —
//! CI runners included. POSIX-only, like the implementation.

#![cfg(unix)]

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
        "perry compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (status {:?}):\nstdout: {}\nstderr: {}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn node_pty_spawn_echo_resize_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The marker is assembled at runtime ("pty_" + "hello") so the terminal's
    // local echo of the typed command can never satisfy the assertion — only
    // the shell's actual output can.
    let out = compile_and_run(
        dir.path(),
        r#"
import { spawn } from "node-pty";

const term = spawn("sh", [], {
  name: "xterm-256color",
  cols: 80,
  rows: 24,
  cwd: process.cwd(),
  env: process.env as Record<string, string>,
});

console.log("PID_POSITIVE:" + (term.pid > 0));
console.log("COLS:" + term.cols + " ROWS:" + term.rows);
console.log("PROCESS:" + term.process);

let buffer = "";
const sub = term.onData((d: string) => {
  buffer += d;
});
console.log("DISPOSABLE:" + (typeof sub.dispose === "function"));

term.onExit((e: { exitCode: number; signal?: number }) => {
  const marker = "pty_" + "hello";
  console.log("SAW_MARKER:" + buffer.includes(marker));
  console.log("EXIT_CODE:" + e.exitCode);
  console.log("EXIT_SIGNAL:" + (e.signal === undefined ? "none" : e.signal));
});

term.resize(100, 40);
console.log("RESIZED:" + term.cols + "x" + term.rows);

term.write('echo "pty_$(echo hello)"\n');
setTimeout(() => {
  term.write("exit\n");
}, 400);
"#,
    );
    assert!(out.contains("PID_POSITIVE:true"), "output: {out}");
    assert!(out.contains("COLS:80 ROWS:24"), "output: {out}");
    assert!(out.contains("PROCESS:sh"), "output: {out}");
    assert!(out.contains("DISPOSABLE:true"), "output: {out}");
    assert!(out.contains("RESIZED:100x40"), "output: {out}");
    assert!(out.contains("SAW_MARKER:true"), "output: {out}");
    assert!(out.contains("EXIT_CODE:0"), "output: {out}");
    assert!(out.contains("EXIT_SIGNAL:none"), "output: {out}");
}

#[test]
fn lydell_alias_kill_sigterm_reports_signal() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `sleep`, not a shell: an *interactive* shell ignores SIGTERM, so the
    // signal would never terminate it.
    let out = compile_and_run(
        dir.path(),
        r#"
import * as pty from "@lydell/node-pty";

const term = pty.spawn("sleep", ["30"], {
  name: "xterm-256color",
  cols: 80,
  rows: 24,
});

console.log("PID_POSITIVE:" + (term.pid > 0));

term.onExit((e: { exitCode: number; signal?: number }) => {
  console.log("EXIT_SIGNAL:" + (e.signal === undefined ? "none" : e.signal));
});

setTimeout(() => {
  term.kill("SIGTERM");
}, 200);
"#,
    );
    assert!(out.contains("PID_POSITIVE:true"), "output: {out}");
    assert!(out.contains("EXIT_SIGNAL:15"), "output: {out}");
}

#[test]
fn dynamic_import_node_pty_namespace_spawn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
async function main() {
  const nodePty = await import("node-pty");
  const term = nodePty.spawn("sh", [], {
    name: "xterm-256color",
    cols: 80,
    rows: 24,
  });
  console.log("DYN_PID_POSITIVE:" + (term.pid > 0));
  let buf = "";
  term.onData((d: string) => {
    buf += d;
  });
  term.onExit((e: { exitCode: number }) => {
    const marker = "dyn_" + "roundtrip";
    console.log("DYN_SAW_MARKER:" + buf.includes(marker));
    console.log("DYN_EXIT:" + e.exitCode);
  });
  term.write('echo "dyn_$(echo roundtrip)"\n');
  setTimeout(() => term.write("exit\n"), 400);
}
main();
"#,
    );
    assert!(out.contains("DYN_PID_POSITIVE:true"), "output: {out}");
    assert!(out.contains("DYN_SAW_MARKER:true"), "output: {out}");
    assert!(out.contains("DYN_EXIT:0"), "output: {out}");
}
