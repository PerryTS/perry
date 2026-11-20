import { performance } from "node:perf_hooks";
// mark(name, { detail }) structured-clones detail: the stored value deep-equals
// the input but is a distinct object reference.
const d = { x: 1 };
const m = performance.mark("c", { detail: d });
console.log("deep equal:", JSON.stringify(m.detail) === JSON.stringify(d));
console.log("distinct ref:", m.detail !== d);
