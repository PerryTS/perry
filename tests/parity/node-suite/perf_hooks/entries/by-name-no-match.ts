import { performance } from "node:perf_hooks";
// getEntriesByName / getEntriesByType return an empty array when nothing matches.
performance.mark("a");
console.log("no name:", performance.getEntriesByName("nope").length);
console.log("no type:", performance.getEntriesByType("measure").length);
