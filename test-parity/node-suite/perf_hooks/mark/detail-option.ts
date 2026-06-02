import { performance } from "node:perf_hooks";
// mark(name, { detail }) attaches a (structured-cloned) detail value.
const m = performance.mark("d", { detail: { x: 1 } });
console.log("name:", m.name);
console.log("detail:", m.detail);
