//! Regression test for #9324.
//!
//! Two halves of one production failure. A Hono service on
//! `@hono/node-server` exited every 30 seconds with
//!
//! ```text
//! TypeError: is not iterable
//! ```
//!
//! The throw was `for (const ws of wss.clients)` inside a `setInterval`
//! heartbeat, where the `clients` read produced `undefined`. Because the
//! TypeError named no subject at all, the report attributed it to an unrelated
//! frame inside the adapter (`@hono/node-server`'s response `close` handler)
//! and the real line was never found.
//!
//! 1. Every `is not iterable` TypeError must NAME its subject, the way Node
//!    does (`undefined is not iterable`, `null is not iterable`). A bare,
//!    subject-less message is unattributable — that is the whole defect.
//! 2. `WebSocketServer.clients` must be the live `Set` on the DYNAMIC property
//!    path too: an `any` alias, a computed key, or a helper taking the server
//!    as an untyped parameter — which is how every compiled npm package reads
//!    it, since a published bundle carries no types. #9335 exposed `clients`
//!    only for a statically-typed receiver.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
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
        .arg("--no-cache")
        .env_remove("PERRY_NO_AUTO_OPTIMIZE")
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
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
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Pre-fix EVERY line below read `is not iterable` with nothing in front of
/// it. `null`, the array destructure and the spread now match Node's message
/// byte for byte; the member read names the VALUE (`undefined`) where Node
/// names the source text (`holder.clients`) — both identify the receiver,
/// which is what a bare message could not do.
#[test]
fn not_iterable_typeerror_names_its_subject() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function message(f: () => void): string {
  try {
    f();
    return "NO THROW";
  } catch (e) {
    return (e as Error).message;
  }
}

// The exact #9324 shape: a member read that resolves to nothing, iterated.
const holder: { clients?: unknown } = {};
console.log(message(() => { for (const _x of holder.clients as any) void _x; }));
console.log(message(() => { for (const _x of null as any) void _x; }));
console.log(message(() => { const [_a] = undefined as any; void _a; }));
console.log(message(() => { void [...(undefined as any)]; }));
"#,
    );

    assert_eq!(
        stdout,
        "undefined is not iterable\n\
         null is not iterable\n\
         undefined is not iterable\n\
         undefined is not iterable\n"
    );
    // Guard the defect itself, not just the current wording: a subject-less
    // message must never come back.
    assert!(
        !stdout.lines().any(|line| line.trim() == "is not iterable"),
        "a bare, subject-less 'is not iterable' survived:\n{stdout}"
    );
}

/// #9335 exposed `clients` only for a statically-typed receiver. MB24's own
/// heartbeat is typed and stopped crashing, but any untyped read — which is
/// what a compiled npm package emits — still produced `undefined` and put the
/// 30-second crash straight back.
#[test]
fn websocket_server_clients_resolves_on_the_dynamic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { WebSocketServer } from "ws";

// `{ noServer: true }` is the shape the reporting service uses (#9324);
// #9335's own test covers the `{ port: 0 }` arm.
const wss = new WebSocketServer({ noServer: true });

const anyAlias: any = wss;
const key = "clients";
function viaUntypedParam(server: any): any {
  return server.clients;
}

console.log(Object.prototype.toString.call(wss.clients));
console.log(Object.prototype.toString.call(anyAlias.clients));
console.log(Object.prototype.toString.call((wss as any)[key]));
console.log(Object.prototype.toString.call(viaUntypedParam(wss)));

// The heartbeat loop that killed the process. Iterating the dynamic read
// must not throw.
let count = 0;
for (const _client of anyAlias.clients) count += 1;
console.log("iterated " + count);

// A `noServer` WebSocketServer still holds the event loop open.
process.exit(0);
"#,
    );

    assert_eq!(
        stdout,
        "[object Set]\n[object Set]\n[object Set]\n[object Set]\niterated 0\n"
    );
}
