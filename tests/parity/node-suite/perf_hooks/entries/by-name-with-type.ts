import { performance } from "node:perf_hooks";
// getEntriesByName(name, type) filters by both name and entryType.
performance.mark("dup");
performance.mark("dup", { startTime: 1 });
console.log("by name only:", performance.getEntriesByName("dup").length);
console.log("by name+mark:", performance.getEntriesByName("dup", "mark").length);
console.log("by name+measure:", performance.getEntriesByName("dup", "measure").length);
