import { performance } from "node:perf_hooks";
// The mark name is coerced to a string (Node: `${name}`).
console.log("undefined:", performance.mark(undefined as any).name);
console.log("number:", performance.mark(1 as any).name);
console.log("boolean:", performance.mark(true as any).name);
console.log("null:", performance.mark(null as any).name);
