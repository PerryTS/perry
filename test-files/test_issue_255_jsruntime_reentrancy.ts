// Issue #255: when a Perry closure passed to a JS-imported function
// (via JsCreateCallback) reads a property from a JS object passed in as
// a callback argument, perry-jsruntime panics with "RefCell already
// borrowed" then "active scope can't be dropped". The closure-marshaling
// itself works (Phase 2B / PR #254) — the panic is on the runtime side
// when js_get_property re-enters with_runtime + creates a new V8 scope
// while the trampoline's scope is still active.
//
// Fix: thread-local REENTRY_SCOPE_PTR stashed by the trampoline, picked
// up by js_handle_object_get_property to reuse the trampoline's existing
// scope instead of creating a conflicting one. Plus thread-local
// REENTRY_PTR for the with_runtime borrow re-entry.

import { makeDispatcher } from "./fixtures/issue_255_jsmod.js";

const d = makeDispatcher();

// Single property read inside callback — the user's exact #248 repro.
d.callWithFrame((ctx: any) => {
  console.log("frame: deltaTime =", ctx.deltaTime);
});

// Multiple property reads, including a nested object read.
d.callWithEvent((ev: any) => {
  console.log("event:", ev.type, "at", ev.x, ev.y);
  console.log("event target id:", ev.target.id);
});

// Same callback fired twice — verifies the trampoline's stash/restore
// guard handles repeated invocations (each invocation gets a fresh
// scope from V8; the guard must clear REENTRY_SCOPE_PTR cleanly between
// them so the second call sees a fresh stash).
d.callTwice((data: any) => {
  console.log("count =", data.count);
});

// Captured-variable mutation from inside callback that ALSO reads a
// property — exercises the full re-entrant chain (with_runtime borrow
// re-entry + V8 scope reuse + capture writeback).
let total = 0;
d.callWithFrame((ctx: any) => {
  total += ctx.deltaTime;
});
console.log("total after capture+read:", total);

console.log("done");
