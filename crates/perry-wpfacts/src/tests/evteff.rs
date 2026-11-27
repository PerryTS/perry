//! `EventTarget.prototype.addEventListener` / `removeEventListener` declare
//! their effects beside the runtime body (event_target.rs
//! `bound_listener_call`): the receiver's brand check reads its
//! `constructor`, the type is ToString'd, the options' `capture` / `once` /
//! `signal` are read, and the listener is kept in an internal slot of the
//! receiver that the declared dispatchers (`dispatchEvent`, the abort
//! events) call with the target as `this` and the event, or, for a listener
//! object, its `handleEvent` with the listener as `this`. Before, the
//! methods were opaque: the listener was handed to unknown code (⊤ calls).
use super::*;

fn stored(prelude: &str, read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "declare function external(x: unknown): void;\nclass H {{ v: any = 0; }}\nclass D {{ q = 1; }}\nconst g: any = {{ v: 0, w: 0 }};\n{prelude}\nconst h = new H();\nh.v = {read};\nconsole.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

fn has_d(k: &crate::derive::KindFact) -> bool {
    k.objs.iter().any(|o| o.ends_with(":D"))
}

/// Red on 22c1d72198: the realm method on a host object (three.js's merged
/// `texture` holds `performance`): the listener escaped, `e` was ⊤.
#[test]
fn performance_listener_is_kept_not_escaped() {
    let k = stored(
        "const t: any = performance; t.addEventListener(\"x\", function (this: any, e: any) { g.v = e; });",
        "g.v",
    );
    assert!(!k.top, "{k:?}");
}

/// Red on 22c1d72198: dispatchEvent calls the kept listener with the
/// target as `this` and the event.
#[test]
fn dispatch_calls_the_listener_with_target_and_event() {
    let p = "const et = new EventTarget(); et.addEventListener(\"x\", function (this: any, e: any) { g.v = new D(); g.w = this; }); et.dispatchEvent(new Event(\"x\"));";
    let k = stored(p, "g.v");
    assert!(!k.top && has_d(&k), "{k:?}");
    let k = stored(p, "g.w");
    assert!(!k.top && k.builtin_objs > 0, "{k:?}");
}

/// A listener object: its `handleEvent`, with the listener as `this`.
#[test]
fn dispatch_calls_handle_event_of_a_listener_object() {
    let p = "const et = new EventTarget(); const q: any = { e: 0 }; const l: any = { handleEvent(e: any) { g.v = new D(); g.w = this; q.e = e; } }; et.addEventListener(\"x\", l); et.dispatchEvent(new Event(\"x\"));";
    let k = stored(p, "g.v");
    assert!(!k.top && has_d(&k), "{k:?}");
    let k = stored(p, "g.w");
    assert!(!k.top, "{k:?}");
    // Red on 22c1d72198 (the listener escaped: unknown code called it
    // with a top event): the event is the dispatched one.
    let k = stored(p, "q.e");
    assert!(!k.top, "{k:?}");
}

/// Red on 22c1d72198: removeEventListener keeps and calls nothing.
#[test]
fn remove_keeps_nothing() {
    let k = stored(
        "const t: any = performance; t.removeEventListener(\"x\", function (e: any) { g.v = e; });",
        "g.v",
    );
    assert!(!k.top, "{k:?}");
}

/// A merged receiver (a realm object or a program EventTarget instance)
/// that the program dispatches on: the listener runs, nothing is ⊤.
#[test]
fn merged_receiver_dispatch() {
    let p = "const et = new EventTarget(); const o: any = Math.random() > 2 ? performance : et; o.addEventListener(\"x\", (e: any) => { g.v = new D(); g.w = e; }); et.dispatchEvent(new Event(\"x\"));";
    let k = stored(p, "g.v");
    assert!(!k.top && has_d(&k), "{k:?}");
    // Red on 22c1d72198 (the listener escaped: called with a top event).
    let k = stored(p, "g.w");
    assert!(!k.top, "{k:?}");
}

/// Control: a non-callable listener is kept but never called (the runtime
/// ignores a primitive one); no ⊤.
#[test]
fn primitive_listener_control() {
    let k = stored(
        "const et = new EventTarget(); et.addEventListener(\"x\", 5 as any); et.dispatchEvent(new Event(\"x\")); g.v = new D();",
        "g.v",
    );
    assert!(!k.top && has_d(&k), "{k:?}");
}

/// Control: the type's ToString runs the program's `toString`.
#[test]
fn the_type_to_string_runs() {
    let k = stored(
        "const et = new EventTarget(); et.addEventListener({ toString() { g.v = new D(); return \"x\"; } } as any, () => {});",
        "g.v",
    );
    assert!(has_d(&k), "{k:?}");
}

/// Control: the options' getters run (`once`).
#[test]
fn the_options_getter_runs() {
    let k = stored(
        "const et = new EventTarget(); et.addEventListener(\"x\", () => {}, { get once() { g.v = new D(); return false; } } as any);",
        "g.v",
    );
    assert!(has_d(&k), "{k:?}");
}

/// Control: the receiver's brand check reads `constructor` (a getter runs).
#[test]
fn the_brand_check_reads_constructor() {
    let k = stored(
        "const o: any = { get constructor() { g.v = new D(); return Object; } }; EventTarget.prototype.addEventListener.call(o, \"x\", () => {});",
        "g.v",
    );
    assert!(has_d(&k), "{k:?}");
}

/// Control: AbortController.prototype.abort fires the signal's listeners.
#[test]
fn abort_calls_signal_listeners() {
    let k = stored(
        "const c = new AbortController(); c.signal.addEventListener(\"abort\", (e: any) => { g.v = new D(); }); c.abort();",
        "g.v",
    );
    assert!(has_d(&k), "{k:?}");
}

/// Control: AbortSignal.timeout's timer fires the result's listeners.
#[test]
fn timeout_calls_signal_listeners() {
    let k = stored(
        "AbortSignal.timeout(1).addEventListener(\"abort\", (e: any) => { g.v = new D(); });",
        "g.v",
    );
    assert!(has_d(&k), "{k:?}");
}

/// Control: a BroadcastChannel's listener is called by message delivery
/// from other channels (no declared dispatcher): it must still run.
#[test]
fn broadcast_channel_listener_runs() {
    let k = stored(
        "const bc = new BroadcastChannel(\"c\"); bc.addEventListener(\"message\", (e: any) => { g.v = new D(); });",
        "g.v",
    );
    assert!(has_d(&k) || k.top, "{k:?}");
}

/// The event goes to every record of the target's bag: the program's
/// listeners (the declared calls) and the records added by code that holds
/// the target (`node:events` once/on, an opaque call). The event is kept in
/// an internal slot of the target, so it leaves when the target does. A
/// target unknown code does not hold: the event's own field stays exact.
#[test]
fn the_event_stays_with_an_unescaped_target() {
    let k = stored(
        "const et = new EventTarget(); const ev: any = new Event(\"x\"); ev.p = new D(); et.dispatchEvent(ev); g.v = ev.p;",
        "g.v",
    );
    assert!(!k.top && has_d(&k), "{k:?}");
}

/// Control: a target unknown code holds (it may have added its own
/// listener): the dispatched event reaches unknown code.
#[test]
fn an_escaped_target_hands_the_event_out() {
    let k = stored(
        "const et = new EventTarget(); external(et); const ev: any = new Event(\"x\"); ev.p = new D(); et.dispatchEvent(ev); g.v = ev.p;",
        "g.v",
    );
    assert!(k.top, "{k:?}");
}
