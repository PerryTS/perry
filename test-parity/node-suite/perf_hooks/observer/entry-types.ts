import { PerformanceObserver } from "node:perf_hooks";
// PerformanceObserver.supportedEntryTypes is a frozen array of strings, and
// observe()/disconnect() are instance methods.
const obs = new PerformanceObserver(() => {});
console.log("observe:", typeof obs.observe);
console.log("disconnect:", typeof obs.disconnect);
console.log("takeRecords:", typeof obs.takeRecords);
console.log("supportedEntryTypes:", Array.isArray(PerformanceObserver.supportedEntryTypes));
console.log("includes mark:", PerformanceObserver.supportedEntryTypes.includes("mark"));
console.log("includes measure:", PerformanceObserver.supportedEntryTypes.includes("measure"));
