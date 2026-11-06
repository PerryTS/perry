import { monitorEventLoopDelay } from "node:perf_hooks";
// monitorEventLoopDelay() returns an IntervalHistogram with enable/disable +
// stats accessors.
const h = monitorEventLoopDelay();
console.log("enable:", typeof h.enable === "function");
console.log("disable:", typeof h.disable === "function");
console.log("mean:", typeof h.mean === "number");
