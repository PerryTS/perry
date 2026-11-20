import * as traceEvents from "node:trace_events";
import traceEventsDefault from "node:trace_events";

const tracing = traceEvents.createTracing({ categories: ["node", "v8"] });

console.log("module keys:", Object.keys(traceEvents).sort().join(","));
console.log("default mirrors:", traceEvents.default.createTracing === traceEvents.createTracing);
console.log("default import mirrors:", traceEventsDefault.createTracing === traceEvents.createTracing);
console.log("constructor:", tracing.constructor.name);
console.log(
  "function lengths:",
  traceEvents.createTracing.length,
  traceEvents.getEnabledCategories.length,
  tracing.enable.length,
  tracing.disable.length,
);
console.log("own keys:", Object.keys(tracing).join(","));
console.log("categories:", tracing.categories);
console.log("enabled:", tracing.enabled);
console.log("methods:", typeof tracing.enable, typeof tracing.disable);
