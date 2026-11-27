//! PERFNOW58/61 (2026-11-21): the two c17 realm-stop chains of the chess
//! ladder (pairs /root/LADDER23/repro/pn57_{A,B}_{bad,good}.ts).
//! - HOSTEV: a Worker event other than "message" is worker_threads.rs
//!   `event_object`'s plain record of five own fields; it holds neither the
//!   Worker nor the clone tree under an any key, so a listener's read of
//!   another key (`e.foo`) is the Object.prototype chain, not the Worker
//!   (whose `then` a resolve would call).
//! - WGPOST: the bare `postMessage(v)` of a module a Worker runs is the
//!   Worker global's own native, Worker.postMessage's structured clone; a
//!   program store under the name (named, or under a key that may spell
//!   it) is called as well.
use super::w3l::solve;

const EMPTY: &str = "export {};\n";

// The main module of pair A: the global web Worker. The type-only import
// gives the harness the import it resolves the Worker's entry through (the
// driver's own is type-only too); it binds nothing at run time.
fn a_main(body: &str, ev: &str) -> String {
    format!(
        "import type {{}} from \"./w.ts\";\n\
         const worker = new Worker(new URL(\"./w.ts\", import.meta.url), {{ type: \"module\" }});\n\
         const p = new Promise((resolve) => {{\n\
           worker.addEventListener(\"{ev}\", (e: any) => {{\n{body}  }});\n\
         }});\n\
         p.then((v) => console.log(v));\n\
         worker.postMessage(1);\n"
    )
}

fn has(tops: &[String], s: &str) -> bool {
    tops.iter().any(|t| t.contains(s))
}

// HOSTEV (RED before): a key the event record does not own; the any-key
// field handed it the Worker, whose `then` the resolve calls.
#[test]
fn hostev_message_listener_other_key_holds_nothing() {
    let main = a_main("    resolve(e.foo);\n", "message");
    let (tops, held, side, _) = solve(&[("main", &main), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert_eq!(side, vec![false, true]);
    assert!(!has(&tops, "Worker.then"), "{tops:?}");
    assert!(!held, "the realm is not held: {tops:?}");
}

// HOSTEV (RED before): the same read in an "error" listener.
#[test]
fn hostev_error_listener_other_key_holds_nothing() {
    let main = a_main("    resolve(e.foo);\n", "error");
    let (tops, held, _, _) = solve(&[("main", &main), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "the realm is not held: {tops:?}");
}

// HOSTEV (RED before): `type` is a string, `defaultPrevented` false:
// resolving with either adopts no thenable.
#[test]
fn hostev_event_fields_are_the_runtime_record() {
    let main = a_main("    resolve([e.type, e.defaultPrevented]);\n", "error");
    let (tops, held, _, _) = solve(&[("main", &main), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "the realm is not held: {tops:?}");
}

// Chain W (PERFNOW77/79, c17 and c23 on a base before HOSTEV): an "error"
// event resolved and a method called on it. worker_pump.rs hands a web
// listener of "error" `event_object(event, worker, None)` (data only for
// "message"): the five own fields, no any key, so the resolve's `then` is
// the Object.prototype chain and `e.type` is a string.
#[test]
fn hostev_error_event_resolved_and_method_called_holds_nothing() {
    let main = a_main("    resolve(e);\n    e.type.toUpperCase();\n", "error");
    let (tops, held, _, _) = solve(&[("main", &main), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "the realm is not held: {tops:?}");
}

// Chain W good: the same listener without the method call.
#[test]
fn hostev_error_event_resolved_holds_nothing() {
    let main = a_main("    resolve(e);\n", "error");
    let (tops, held, _, _) = solve(&[("main", &main), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "the realm is not held: {tops:?}");
}

const B_MAIN: &str = "import { Worker } from \"node:worker_threads\";\n\
    new Worker(\"./w.ts\");\n";

// B (RED before WGPOST): the worker module's bare `postMessage(null)`.
#[test]
fn wgpost_worker_bare_post_message_is_the_clone() {
    let w = "postMessage(null);\nexport {};\n";
    let (tops, held, side, _) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert_eq!(side, vec![false, true]);
    assert!(!has(&tops, "Other.postMessage"), "{tops:?}");
    assert!(!held, "the realm is not held: {tops:?}");
}

// B good: the worker module without the call.
#[test]
fn wgpost_pair_good_holds_nothing() {
    let (tops, held, _, _) = solve(&[("main", B_MAIN), ("w", EMPTY)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "{tops:?}");
}

// The clone reads the value's own getters, as Worker.postMessage's does.
#[test]
fn wgpost_clone_runs_the_own_getter() {
    let w = "function f(x: any): void { console.log(x); }\n\
        postMessage({ get v() { f(1); return 1; } });\nexport {};\n";
    let (tops, held, _, ps) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "{tops:?}");
    assert!(ps.len() == 1, "{ps:?}");
}

// A program store under a key that may spell `postMessage` (R-GLOBAL:
// program stores are exact) may have replaced the native: the bare call
// calls the stored function with the argument.
#[test]
fn wgpost_stored_global_is_called() {
    let w = "function f(x: any): void { console.log(x); }\n\
        (globalThis as any)[String(Date.now())] = (v: any) => { f(v); };\n\
        postMessage({ a: 1 });\nexport {};\n";
    let (tops, held, _, ps) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "{tops:?}");
    assert!(
        ps.len() == 1 && ps[0] > 0,
        "the stored function gets the message: {ps:?}"
    );
}

// The same under the name itself.
#[test]
fn wgpost_named_store_is_called() {
    let w = "function f(x: any): void { console.log(x); }\n\
        (globalThis as any).postMessage = (v: any) => { f(v); };\n\
        postMessage({ a: 1 });\nexport {};\n";
    let (tops, held, _, ps) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "{tops:?}");
    assert!(
        ps.len() == 1 && ps[0] > 0,
        "the stored function gets the message: {ps:?}"
    );
}

// REALMVAL5: a bare read in a module a Worker runs is the Worker global's
// own member as the realm declares it (`%WorkerGlobal%.<name>`): `self` the
// global object, the handlers `null`, the natives their function values.
#[test]
fn worker_global_bare_reads_are_the_declared_members() {
    let w = "const s: any = self; const h: any = onmessage; const p: any = postMessage;\n\
        console.log(s === globalThis, h === null, typeof p);\nexport {};\n";
    let (tops, held, side, _) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert_eq!(side, vec![false, true]);
    assert!(!has(&tops, "Worker global"), "{tops:?}");
    assert!(!held, "the realm is not held: {tops:?}");
}

// The natives read as values are called as the bare call calls them: the
// clone for `postMessage`; `close` and `removeEventListener` call nothing.
#[test]
fn worker_global_native_values_run_their_models() {
    let w = "const p: any = postMessage; p({ a: 1 });\n\
        close(); removeEventListener(\"message\", () => {});\n\
        const c: any = close; c();\nexport {};\n";
    let (tops, held, _, _) = solve(&[("main", B_MAIN), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert!(!has(&tops, "Other.close") && !has(&tops, "Other.removeEventListener"), "{tops:?}");
    assert!(!held, "the realm is not held: {tops:?}");
}
