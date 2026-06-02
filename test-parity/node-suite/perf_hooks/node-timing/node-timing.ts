import { performance } from "node:perf_hooks";
// performance.nodeTiming is a PerformanceNodeTiming entry with bootstrap
// milestones.
const nt = performance.nodeTiming;
console.log("is object:", typeof nt === "object" && nt !== null);
console.log("nodeStart number:", typeof nt.nodeStart === "number");
console.log("entryType:", nt.entryType);
