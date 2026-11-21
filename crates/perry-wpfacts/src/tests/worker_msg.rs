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

