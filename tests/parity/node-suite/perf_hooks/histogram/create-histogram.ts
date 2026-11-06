import { createHistogram } from "node:perf_hooks";
// createHistogram() returns a RecordableHistogram supporting record()/min/max.
const h = createHistogram();
console.log("record:", typeof h.record === "function");
h.record(5);
console.log("count:", typeof h.count === "number");
