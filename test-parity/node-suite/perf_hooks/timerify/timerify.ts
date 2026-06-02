import { performance } from "node:perf_hooks";
// performance.timerify(fn) wraps fn so each call records a 'function' timeline
// entry; the wrapper still returns fn's result.
const wrapped = performance.timerify((a: number, b: number) => a + b);
console.log("is function:", typeof wrapped === "function");
console.log("result:", wrapped(2, 3));
