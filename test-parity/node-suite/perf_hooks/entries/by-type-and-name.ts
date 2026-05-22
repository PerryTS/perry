import { performance } from "node:perf_hooks";
// getEntriesByType / getEntriesByName filter the performance timeline.
performance.mark("a");
performance.mark("b");
performance.measure("a to b", "a", "b");
console.log("marks:", performance.getEntriesByType("mark").length);
console.log("measures:", performance.getEntriesByType("measure").length);
console.log("named a:", performance.getEntriesByName("a").length);
console.log("named a to b:", performance.getEntriesByName("a to b").length);
