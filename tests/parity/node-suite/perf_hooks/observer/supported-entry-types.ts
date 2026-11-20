import { PerformanceObserver } from "node:perf_hooks";
// PerformanceObserver.supportedEntryTypes is a static array including the
// user-timing types.
const t = PerformanceObserver.supportedEntryTypes;
console.log("is array:", Array.isArray(t));
console.log("includes mark:", t.includes("mark"));
console.log("includes measure:", t.includes("measure"));
