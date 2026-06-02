import { performance } from "node:perf_hooks";
// The resource-timing buffer management methods exist on performance.
console.log("clearResourceTimings:", typeof performance.clearResourceTimings === "function");
console.log("setResourceTimingBufferSize:", typeof performance.setResourceTimingBufferSize === "function");
