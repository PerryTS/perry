// #9412 — after a `require()` of a builtin, `process.nextTick` callbacks ran
// AFTER promise microtasks instead of before.
//
// The entry file is CommonJS (bare `require`, no top-level `import`/`export`),
// so Node runs it with CommonJS semantics: the nextTick queue is drained before
// the promise-job queue. Perry CJS-wrapped the entry, the wrapper injected a
// synthetic `import { createRequire } from 'node:module'` plus an
// `export default`, and that made codegen mark the entry as an ES module — which
// switches on the (correct, for real ESM) "defer the first tick drain until the
// microtask queue is empty" checkpoint. A CommonJS program then got ESM
// ordering.
//
// `.cts`, deliberately: this repo's package is `"type": "module"`, so Node runs
// a plain `.ts` as an ES module and a bare `require` in one dies with
// `require is not defined` before the ordering can be compared at all. The
// extension names the module goal, so both engines agree on CommonJS. The
// parity runner discovers `.cts` since #9418. Keep this file free of top-level
// `import`/`export`.
//
// (Perry decides the goal from CONTENT, not from the nearest package.json, so
// a plain `.ts` with neither `import`/`export` nor `require` is ticks-first
// for perry and ticks-last for Node under `"type": "module"` — a separate,
// pre-existing module-detection divergence, not this one.)

const path = require("path");

const order: string[] = [];

process.nextTick(() => order.push("tick1"));
Promise.resolve().then(() => order.push("promise1"));
(async () => {
    await null;
    order.push("await1");
})();
process.nextTick(() => order.push("tick2"));
queueMicrotask(() => order.push("queueMicrotask1"));

// A tick scheduled from inside a tick joins the same drain, ahead of the
// microtask queue; a tick scheduled from inside a microtask runs after the
// microtask queue drains.
process.nextTick(() => {
    order.push("tick3");
    process.nextTick(() => order.push("tick3-nested"));
});
Promise.resolve().then(() => {
    order.push("promise2");
    process.nextTick(() => order.push("tick-from-promise"));
});

setTimeout(() => {
    console.log("require worked: " + (path.sep === "/" || path.sep === "\\"));
    console.log("order: " + JSON.stringify(order));

    // Second turn: the same invariant must hold outside the first drain, where
    // no ESM evaluation checkpoint could ever apply.
    const later: string[] = [];
    process.nextTick(() => later.push("tick"));
    Promise.resolve().then(() => later.push("promise"));
    queueMicrotask(() => later.push("queueMicrotask"));
    setTimeout(() => {
        console.log("later: " + JSON.stringify(later));
    }, 10);
}, 20);
