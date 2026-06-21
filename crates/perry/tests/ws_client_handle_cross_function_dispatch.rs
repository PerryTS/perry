//! Regression: the `("ws","Client")` upgrade handle delivered to
//! `server.on("upgrade", (req, wsId, head) => …)` must keep dispatching its
//! `.send()` / `.on()` / `.close()` to the dedicated `js_ws_*_client_i64`
//! runtime even when `wsId` is handed to a helper function.
//!
//! Before the fix, that host-class was tagged only at the upgrade callback's
//! parameter and was NOT propagated across a call boundary: inside a
//! `handleConnection(req, wsId)` helper the parameter was plain/`any`, the
//! codegen dispatch table's `class_filter: Some("Client")` rows no longer
//! matched, and `wsId.send(...)` silently lowered to a generic no-op — the
//! frame was dropped with no error. (The downstream workaround was to inline
//! every `wsId` use in the upgrade callback.)
//!
//! These are codegen-dispatch assertions: a WS network round-trip needs a live
//! socket + client, but the bug is purely *which runtime symbol the call site
//! lowers to*. So we compile to LLVM IR and assert the helper's `wsId.send`
//! lowers to a `call` to `js_ws_send_client_i64` (present after the fix, absent
//! before — verified by toggling the fix).

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` and return the concatenated LLVM IR text of all emitted
/// modules (via `PERRY_SAVE_LL`). Asserts the compile succeeds.
fn compile_to_ir(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    let ll_dir = dir.join("ll");
    std::fs::create_dir_all(&ll_dir).expect("mk ll dir");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        // Emit per-module .ll and force a full (uncached) codegen so the IR is
        // actually written for this build.
        .env("PERRY_SAVE_LL", &ll_dir)
        .env("PERRY_LLVM_KEEP_IR", "1")
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut ir = String::new();
    for entry in std::fs::read_dir(&ll_dir).expect("read ll dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("ll") {
            ir.push_str(&std::fs::read_to_string(&path).expect("read .ll"));
            ir.push('\n');
        }
    }
    assert!(!ir.is_empty(), "no .ll IR was emitted");
    ir
}

/// True if the IR contains a `call` instruction targeting `@<symbol>` (i.e. a
/// real call site, not merely the unconditional `declare`).
fn ir_has_call(ir: &str, symbol: &str) -> bool {
    let needle = format!("@{symbol}(");
    ir.lines()
        .any(|l| l.contains("call ") && l.contains(&needle))
}

/// Number of `call` instructions targeting `@<symbol>`.
fn ir_call_count(ir: &str, symbol: &str) -> usize {
    let needle = format!("@{symbol}(");
    ir.lines()
        .filter(|l| l.contains("call ") && l.contains(&needle))
        .count()
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

/// The reported failure: the ONLY `wsId.send` is inside a helper reached as
/// `pushFrame(wsId, …)`. After the fix it must lower to the Client runtime.
#[test]
fn helper_wsid_send_dispatches_to_client_runtime() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function pushFrame(wsId: any, msg: string) {
  wsId.send(msg);
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });

server.on("upgrade", (req: any, wsId: any, _head: any) => {
  pushFrame(wsId, "hello-from-helper");
});

server.listen(0, () => {});
"#,
    );
    assert!(
        ir_has_call(&ir, "js_ws_send_client_i64"),
        "helper `wsId.send` must dispatch to js_ws_send_client_i64 (the upgrade \
         Client handle), not a silent generic no-op"
    );
}

/// The handle is forwarded through TWO helpers — exercises the transitive
/// fixpoint propagation, not just one hop.
#[test]
fn transitively_forwarded_wsid_send_dispatches_to_client_runtime() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function sendFrame(wsId: any, msg: string) {
  wsId.send(msg);
}
function handleConnection(req: any, wsId: any) {
  sendFrame(wsId, "mount");
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });

server.on("upgrade", (req: any, wsId: any, _head: any) => {
  handleConnection(req, wsId);
});

server.listen(0, () => {});
"#,
    );
    assert!(
        ir_has_call(&ir, "js_ws_send_client_i64"),
        "wsId.send two call-hops away from the upgrade callback must still \
         dispatch to the Client runtime"
    );
}

/// `.on(...)` and `.close()` on the forwarded handle must likewise reach the
/// Client runtime, not just `.send`.
#[test]
fn helper_wsid_on_and_close_dispatch_to_client_runtime() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function wire(wsId: any) {
  wsId.on("message", (m: any) => { wsId.send("echo:" + m); });
  wsId.close();
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });

server.on("upgrade", (req: any, wsId: any, _head: any) => {
  wire(wsId);
});

server.listen(0, () => {});
"#,
    );
    assert!(
        ir_has_call(&ir, "js_ws_on_client_i64"),
        "helper `wsId.on` must dispatch to js_ws_on_client_i64"
    );
    assert!(
        ir_has_call(&ir, "js_ws_close_client_i64"),
        "helper `wsId.close` must dispatch to js_ws_close_client_i64"
    );
    assert!(
        ir_has_call(&ir, "js_ws_send_client_i64"),
        "the nested `wsId.send` (inside the helper's own `.on` callback) must \
         dispatch to js_ws_send_client_i64"
    );
}

/// Gate / no-regression control: a `.send`-bearing parameter that is NOT an
/// upgrade Client handle must keep invoking the value's OWN `send` method — the
/// propagation must not mis-tag arbitrary parameters as `("ws","Client")`.
/// (Also confirms the `createServer`-origin gate: there is no upgrade seed at
/// all here.)
#[test]
fn unrelated_send_param_runs_its_own_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
function deliver(ch: any, msg: string) {
  ch.send(msg);
}
const sink = { send(m: string) { console.log("own-send:" + m); } };
deliver(sink, "hi");
"#,
    );
    assert!(
        out.contains("own-send:hi"),
        "a non-ws object's own `send` must run when passed to a helper (got: {out:?})"
    );
    // And it must NOT have been routed to the ws Client runtime.
    let ir = compile_to_ir(
        dir.path(),
        r#"
function deliver(ch: any, msg: string) {
  ch.send(msg);
}
const sink = { send(m: string) { console.log("own-send:" + m); } };
deliver(sink, "hi");
"#,
    );
    assert!(
        !ir_has_call(&ir, "js_ws_send_client_i64"),
        "a plain object's `.send` must NOT be mis-dispatched to the ws Client runtime"
    );
}

// ─── PR #5493 review fixes — edge cases ──────────────────────────────────────

/// Review #2: a leading TypeScript `this:` param must not shift the hint index;
/// the `wsId` (2nd real param) still dispatches to the Client runtime.
#[test]
fn leading_this_param_keeps_hint_index_aligned() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function deliver(this: any, req: any, wsId: any) {
  wsId.send("hi");
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  deliver(req, wsId);
});
server.listen(0, () => {});
"#,
    );
    assert!(
        ir_has_call(&ir, "js_ws_send_client_i64"),
        "wsId after a TS `this:` param must still dispatch to the Client runtime"
    );
}

/// Review #3: a `createServer` that is NOT a `node:http` import must not seed ws
/// taint — a user factory of the same name is ignored.
#[test]
fn non_http_create_server_does_not_seed_ws_taint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
// A user-defined factory that happens to be named `createServer`.
function createServer(): any {
  return { on(_e: string, _cb: any) {} };
}
function deliver(wsId: any) {
  wsId.send("nope");
}
const server: any = createServer();
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  deliver(wsId);
});
"#,
    );
    assert!(
        !ir_has_call(&ir, "js_ws_send_client_i64"),
        "a non-http `createServer` must not seed the ws Client dispatch"
    );
}

/// Review #4: a nested function that shadows `wsId` must not inherit the outer
/// handle — its callee stays untagged even though it is reached from the
/// upgrade callback's lexical scope.
#[test]
fn shadowing_nested_param_does_not_inherit_taint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function deliver(ch: any) {
  ch.send("shadow");
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  // `later` rebinds `wsId`, so the value flowing into `deliver` is NOT the
  // upgrade handle — `deliver` must not be tagged as a ws Client receiver.
  function later(wsId: any) {
    deliver(wsId);
  }
  later(req);
});
server.listen(0, () => {});
"#,
    );
    assert!(
        !ir_has_call(&ir, "js_ws_send_client_i64"),
        "a shadowing nested param must not propagate the outer Client handle"
    );
}

/// Review #1: two functions named `pushFrame` — only the upgrade-fed top-level
/// declaration is tagged; a same-named nested declaration receiving a non-ws
/// value is keyed by a distinct identity and left untouched (exactly one Client
/// dispatch in the module).
#[test]
fn same_named_function_is_not_cross_tagged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function pushFrame(wsId: any, m: string) {
  wsId.send(m);
}
function elsewhere() {
  function pushFrame(x: any) {
    x.send("nope");
  }
  pushFrame({ send(s: string) { console.log(s); } });
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  pushFrame(wsId, "real");
  elsewhere();
});
server.listen(0, () => {});
"#,
    );
    assert_eq!(
        ir_call_count(&ir, "js_ws_send_client_i64"),
        1,
        "only the upgrade-fed `pushFrame` dispatches to the Client runtime, not \
         the same-named nested declaration"
    );
}

// ─── Polymorphism guard — a helper fed BOTH a ws Client and a non-ws value ───

/// A hint tags a function PARAMETER, fixing the dispatch for that param across
/// EVERY call site. So a helper that is fed the upgrade `wsId` from one caller
/// AND a plain object from another must NOT be tagged — otherwise the plain
/// object's `.send(...)` would silently re-route to `js_ws_send_client_i64`
/// (a no-op handle miss) instead of running the object's own method. The pass
/// must only tag a param it can prove is ALWAYS the ws handle.
#[test]
fn polymorphic_helper_fed_ws_and_nonws_is_not_tagged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

// `emit` is reused for BOTH the ws upgrade handle and a plain emitter.
function emit(target: any, msg: string) {
  target.send(msg);
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  emit(wsId, "ws-bound");
});

// A non-ws caller of the same helper — must keep running the value's own method.
const bus: any = { send(m: string) { console.log("bus:" + m); } };
emit(bus, "not-ws");

server.listen(0, () => {});
"#,
    );
    assert!(
        !ir_has_call(&ir, "js_ws_send_client_i64"),
        "a helper that is ALSO called with a non-ws value must not be tagged as \
         a ws Client receiver (would silently drop the non-ws caller's frame)"
    );
}

/// Transitive demotion: the handle is forwarded `mid` → `inner`, but `mid` is
/// ALSO called with a non-ws value. `mid` is polymorphic, so on that path it
/// delivers a non-ws value to `inner` — therefore NEITHER `mid` nor `inner` may
/// be tagged. Exercises the dependency fixpoint, not just the direct caller.
#[test]
fn transitive_through_polymorphic_intermediate_is_not_tagged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function inner(target: any, msg: string) {
  target.send(msg);
}
function mid(target: any, msg: string) {
  inner(target, msg);
}

const server = createServer((req: any, res: any) => { res.statusCode = 200; res.end("ok"); });
server.on("upgrade", (req: any, wsId: any, _head: any) => {
  mid(wsId, "ws-bound");
});

// `mid` is also fed a non-ws value — taint through it is unsound.
const bus: any = { send(m: string) { console.log("bus:" + m); } };
mid(bus, "not-ws");

server.listen(0, () => {});
"#,
    );
    assert!(
        !ir_has_call(&ir, "js_ws_send_client_i64"),
        "a handle forwarded through a polymorphic intermediate must not tag the \
         downstream helper (transitive demotion)"
    );
}

/// Guardrail for over-correction: a helper that is ONLY ever fed the ws handle
/// (across two distinct upgrade callbacks) must STILL be tagged — the
/// polymorphism guard must not drop a legitimately ws-exclusive helper.
#[test]
fn ws_exclusive_helper_with_multiple_ws_callers_stays_tagged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = compile_to_ir(
        dir.path(),
        r#"
import { createServer } from "node:http";

function push(wsId: any, msg: string) {
  wsId.send(msg);
}

const a = createServer((req: any, res: any) => { res.end("a"); });
const b = createServer((req: any, res: any) => { res.end("b"); });
a.on("upgrade", (req: any, wsId: any, _h: any) => { push(wsId, "from-a"); });
b.on("upgrade", (req: any, wsId: any, _h: any) => { push(wsId, "from-b"); });
a.listen(0, () => {});
b.listen(0, () => {});
"#,
    );
    assert!(
        ir_has_call(&ir, "js_ws_send_client_i64"),
        "a helper fed ONLY ws handles (from multiple upgrade callbacks) must stay tagged"
    );
}
