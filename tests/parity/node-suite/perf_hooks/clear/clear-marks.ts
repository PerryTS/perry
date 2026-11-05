import { performance } from "node:perf_hooks";
// clearMarks() removes mark entries; clearMarks(name) removes just that mark.
performance.mark("a");
performance.mark("b");
console.log("before:", performance.getEntriesByType("mark").length);
performance.clearMarks("a");
console.log("after clear a:", performance.getEntriesByType("mark").length);
performance.clearMarks();
console.log("after clear all:", performance.getEntriesByType("mark").length);
