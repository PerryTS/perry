// Refs #2135 (node:process output diffs): `beforeExit` was registered as
// an event the runtime understood (`process.on("beforeExit", ...)` and
// `js_process_emit` both worked manually), but the codegen-emitted
// event-loop epilogue never fired the synthetic emission, so the
// listener was a dead callback. Node's contract: when the event loop is
// about to exit on its own (no real work pending), emit `beforeExit`
// with the would-be exit code as the single argument; explicit
// `process.exit()` bypasses it.
//
// The runtime now exposes `js_process_emit_before_exit(code)`; codegen
// calls it from the loop-exit block once the loop has drained, then runs
// one more microtask/timer-tick pass so any work the listener queued
// still gets a chance to run before the final ret.

console.log("body");

process.prependListener("beforeExit", () => console.log("listener 0 (prepended)"));
process.on("beforeExit", () => console.log("listener 1"));
process.on("beforeExit", (code) => console.log("listener 2, code=" + code));
