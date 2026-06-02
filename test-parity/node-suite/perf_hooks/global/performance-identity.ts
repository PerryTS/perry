import { performance } from "node:perf_hooks";
// The named import and the global are the same object (Node guarantee:
// globalThis.performance === require("perf_hooks").performance).
console.log("same object:", (globalThis as any).performance === performance);
console.log("global now is fn:", typeof (globalThis as any).performance.now);
