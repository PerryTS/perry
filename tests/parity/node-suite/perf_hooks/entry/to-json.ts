import { performance } from "node:perf_hooks";
// PerformanceEntry#toJSON() serializes the entry's fields.
const m = performance.mark("x", { startTime: 3 });
console.log("toJSON is function:", typeof m.toJSON === "function");
const j = m.toJSON();
console.log("name:", j.name);
console.log("entryType:", j.entryType);
console.log("startTime:", j.startTime);
