// #9412 control — the ESM half of the `process.nextTick` ordering contract.
//
// Node defers the first `process.nextTick` drain of an ES-module entry until
// the promise-job queue has drained (module evaluation itself runs inside the
// module job's promise chain), so an ESM entry prints
// ["promise1","await1","tick1"] while a CommonJS entry prints
// ["tick1","promise1","await1"].
//
// Perry models that with a one-shot "ESM evaluation checkpoint". #9412 was that
// the checkpoint also fired for a CommonJS entry, because the CJS wrapper's
// synthetic `node:module` import made codegen classify the entry as ESM. The
// CommonJS side is covered by
// `test-files/test_gap_9412_require_builtin_tick_order.cts`; this
// fixture pins the ESM side so the fix cannot be "stop deferring, always".
//
// This file has a real top-level `import`, so it is a genuine ES module under
// both Node and perry.
import * as nodePath from "node:path";

const order: string[] = [];

process.nextTick(() => order.push("tick1"));
Promise.resolve().then(() => order.push("promise1"));
(async () => {
    await null;
    order.push("await1");
})();
process.nextTick(() => order.push("tick2"));
queueMicrotask(() => order.push("queueMicrotask1"));

setTimeout(() => {
    console.log("import worked: " + (nodePath.sep === "/" || nodePath.sep === "\\"));
    console.log("esm entry order: " + JSON.stringify(order));

    // After the one-shot evaluation checkpoint is spent, ticks lead again —
    // in an ES module exactly as in CommonJS.
    const later: string[] = [];
    process.nextTick(() => later.push("tick"));
    Promise.resolve().then(() => later.push("promise"));
    queueMicrotask(() => later.push("queueMicrotask"));
    setTimeout(() => {
        console.log("esm later: " + JSON.stringify(later));
    }, 10);
}, 20);
