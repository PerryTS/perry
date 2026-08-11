//! The `node:events` module-helper name list lives in THREE places, and they
//! must agree. This test is the only thing that makes a disagreement loud.
//!
//! 1. **here** — the `has_receiver: false` `events` rows of [`NET_EVENTS_ROWS`],
//!    which serve the STATIC call `events.listenerCount(e, "x")`;
//! 2. `nm_dispatch_events` in perry-runtime, which serves every INDIRECT form
//!    (captured value, type-erased receiver, spread call) and can only reach
//!    the stdlib helpers through a registered function pointer;
//! 3. `js_events_native_dispatch` in perry-stdlib, the other end of that
//!    pointer.
//!
//! When (1) had rows that (2) and (3) did not, the static call was correct and
//! every indirect form silently answered `undefined` — `const c =
//! events.listenerCount; c(e, "x")` returned `undefined` where node returns a
//! count. That is invisible to a static-form test, which is why the drift
//! survived; it took a spread call routed onto the dynamic path to surface it.
//!
//! A new module-level `events` export therefore has to be CLASSIFIED below, not
//! merely added to the table. The test fails on an unclassified row rather than
//! letting it ship with a dead dynamic path.

#![cfg(test)]

use super::net_events::NET_EVENTS_ROWS;

/// Routed by `nm_dispatch_events` to perry-stdlib's `js_events_native_dispatch`.
/// Keep byte-identical to the match arm there and to the `match name` arms in
/// the stdlib bridge.
const ROUTED_TO_STDLIB_BRIDGE: &[&str] = &[
    "listenerCount",
    "once",
    "on",
    "getEventListeners",
    "getMaxListeners",
    "setMaxListeners",
    "addAbortListener",
];

/// Answered by perry-runtime directly — no stdlib bridge needed.
/// `init` is a no-op returning `undefined`; `EventEmitterAsyncResource` throws
/// the "cannot be invoked without 'new'" TypeError.
const ANSWERED_BY_RUNTIME: &[&str] = &["init", "EventEmitterAsyncResource"];

/// Deliberately unrouted: `events.EventEmitter` is a CONSTRUCTOR. `new`-ing it
/// dynamically goes through `JS_NATIVE_EVENTS_CONSTRUCT`, a separate pointer;
/// calling it as a plain function through the dynamic path is not supported and
/// is not what the row serves.
const NOT_A_DYNAMIC_MODULE_CALL: &[&str] = &["EventEmitter"];

fn module_level_events_methods() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = NET_EVENTS_ROWS
        .iter()
        .filter(|row| row.module == "events" && !row.has_receiver)
        .map(|row| row.method)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[test]
fn every_events_module_helper_is_classified_for_dynamic_dispatch() {
    for method in module_level_events_methods() {
        let classified = ROUTED_TO_STDLIB_BRIDGE.contains(&method)
            || ANSWERED_BY_RUNTIME.contains(&method)
            || NOT_A_DYNAMIC_MODULE_CALL.contains(&method);
        assert!(
            classified,
            "`events.{method}` has a static NativeModSig row but no decision about the \
             dynamic path. Add it to `nm_dispatch_events` (perry-runtime) AND \
             `js_events_native_dispatch` (perry-stdlib), then list it in \
             ROUTED_TO_STDLIB_BRIDGE — or justify it in one of the other two lists. \
             Leaving it unclassified ships a helper whose captured / type-erased / \
             spread forms silently return `undefined`."
        );
    }
}

#[test]
fn classification_lists_do_not_name_rows_that_no_longer_exist() {
    // A stale entry is the mirror-image failure: it makes the test above pass
    // for a name the table dropped, so the next real addition slips through
    // unnoticed.
    let existing = module_level_events_methods();
    for list in [
        ROUTED_TO_STDLIB_BRIDGE,
        ANSWERED_BY_RUNTIME,
        NOT_A_DYNAMIC_MODULE_CALL,
    ] {
        for method in list {
            assert!(
                existing.contains(method),
                "`events.{method}` is classified here but has no `has_receiver: false` \
                 row in NET_EVENTS_ROWS — delete the stale entry."
            );
        }
    }
}
