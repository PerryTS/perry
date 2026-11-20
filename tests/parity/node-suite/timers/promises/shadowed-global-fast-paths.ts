import { setImmediate, setTimeout } from "node:timers/promises";

async function report(label: string, fn: () => Promise<unknown>) {
  try {
    const value = await fn();
    console.log(label + " resolved", typeof value, JSON.stringify(value));
  } catch (err: any) {
    console.log(label + " error", err?.message || String(err));
  }
}

// Regression for #1317: these named imports intentionally shadow the global
// timer functions. Perry must route them through the node:timers/promises
// import thunk, not the global timer callback fast paths.
await report("setTimeout", () => setTimeout(1, { a: 1 }, { ref: true }));
await report("setImmediate", () => setImmediate({ b: 2 }, { ref: true }));
