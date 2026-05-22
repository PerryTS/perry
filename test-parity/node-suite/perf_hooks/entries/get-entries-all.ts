import { performance } from "node:perf_hooks";
// getEntries() returns the whole timeline (marks + measures), unfiltered.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 5 });
performance.measure("ab", "a", "b");
const all = performance.getEntries();
console.log("total:", all.length);
console.log("types:", all.map((e) => e.entryType).join(","));
