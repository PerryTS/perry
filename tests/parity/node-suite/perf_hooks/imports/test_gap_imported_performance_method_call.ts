import { performance } from "node:perf_hooks";
// The imported `performance` is the global object, and calls through it (or
// an alias of it) dispatch like calls on the global. (No `globalThis` read
// here: that installs the global surfaces and would hide a missing install.)
const p = performance;
console.log(typeof p.now(), typeof performance.now());
console.log(p.now() >= 0, typeof p.timeOrigin);
