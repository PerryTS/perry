import { performance } from "node:perf_hooks";
// clearMeasures(name) removes only the same-named measures.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 5 });
performance.measure("m1", "a", "b");
performance.measure("m2", "a", "b");
performance.clearMeasures("m1");
console.log("remaining:", performance.getEntriesByType("measure").length);
console.log("m2 survives:", performance.getEntriesByName("m2", "measure").length);
