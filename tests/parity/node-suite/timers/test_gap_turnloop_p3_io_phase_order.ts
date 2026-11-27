// turnloop P3 — the poll phase and the check phase that follows it.
//
// Measured against the pinned oracle, five runs each; only the orderings that
// were byte-identical on all five are asserted.
//
//   1. **Inside an I/O callback, `setImmediate` beats `setTimeout`.** The poll
//      phase is followed by the check phase in the SAME loop iteration, while a
//      timeout scheduled beside it waits for the next iteration's timers phase.
//      (Node: stable 5/5; also the shape that made this the canonical Node
//      event-loop question.)
//   2. **`process.nextTick` and the microtask queue drain before either.**
//      Stable across all 25 runs of the oracle sweep.
//
// Deliberately NOT asserted, because the oracle showed them unstable:
//   - a top-level `setImmediate` against a top-level `fs.readFile` callback
//     (Node 26.12.1 runs the read's callback first in about 1 run of 4);
//   - the same race with a cheap `fs.stat('.')` (3/5 vs 2/5);
//   - how many loop turns a top-level `fs.readFile` callback takes (5-7), and
//     therefore anything that races a second read against a timer;
//   - `setTimeout(…, 0)` vs `setImmediate` at top level, or from inside a
//     running immediate.

import * as fs from "node:fs";

const log: string[] = [];
const toplevel: string[] = [];
const here = import.meta.filename;

setImmediate(() => toplevel.push("immediate-a"));
fs.readFile(here, () => {
  log.push("io:callback");

  // (1) and (2) — everything below is scheduled from inside the I/O callback.
  setTimeout(() => log.push("io:timeout"), 0);
  setImmediate(() => log.push("io:immediate"));
  process.nextTick(() => log.push("io:tick"));
  Promise.resolve().then(() => log.push("io:promise"));
  queueMicrotask(() => log.push("io:queueMicrotask"));
});
setImmediate(() => toplevel.push("immediate-b"));

setTimeout(() => {
  console.log("order:");
  for (const line of log) console.log("  " + line);
  console.log("toplevel:", toplevel.join(","));
}, 120);
