import { performance } from "node:perf_hooks";
// measure(name, { detail }) structured-clones detail: deep-equal but a distinct
// reference (parallel to mark's detail handling).
const d = { a: 1 };
const m = performance.measure("mc", { detail: d, start: 0, end: 1 });
console.log("deep equal:", JSON.stringify(m.detail) === JSON.stringify(d));
console.log("distinct ref:", m.detail !== d);
