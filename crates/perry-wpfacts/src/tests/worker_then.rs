//! PN66 WTHEN (LADDER c17-A3): a promise resolved with a Worker. The resolve
//! function reads the value's `then` (ECMA-262 27.2.1.3.2 step 9). The
//! runtime Worker (`worker_object_with_id`, worker_surface.rs) owns no
//! `then`, nor does its [[Prototype]] chain (worker_threads
//! `Worker.prototype` -> `EventEmitter.prototype` -> `Object.prototype`;
//! probe pn66/probe/wk.ts: `"then" in worker` is false), so with no program
//! store under `then` the promise fulfils with the Worker and nothing runs.
use super::w3l::solve;

/// `Promise.resolve(worker)` (pair pn62_a3r).
#[test]
fn wthen_promise_resolve_of_a_worker_holds_nothing() {
    let main = "import type {} from \"./w.ts\";\n\
        const worker = new Worker(new URL(\"./w.ts\", import.meta.url), { type: \"module\" });\n\
        const p = Promise.resolve(worker);\n";
    let (tops, held, _, _) = solve(
        &[("main", main), ("w", "export {};\n")],
        &["w.ts"],
        &["./w.ts"],
    );
    assert!(!held, "{tops:?}");
}
