import { performance } from "node:perf_hooks";
// performance.markResourceTiming(info) records a resource-timing entry
// (paired with clearResourceTimings / setResourceTimingBufferSize).
console.log("is function:", typeof performance.markResourceTiming === "function");
