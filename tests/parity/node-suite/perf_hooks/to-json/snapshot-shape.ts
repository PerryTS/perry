import { performance } from "node:perf_hooks";

const json = performance.toJSON();
const nodeTiming = performance.nodeTiming;

console.log("toJSON keys:", Object.keys(json).sort().join(","));
console.log("toJSON elu keys:", Object.keys(json.eventLoopUtilization).sort().join(","));
console.log("toJSON nodeTiming keys:", Object.keys(json.nodeTiming).sort().join(","));
console.log("nodeTiming keys:", Object.keys(nodeTiming).sort().join(","));
console.log("uv keys:", Object.keys(nodeTiming.uvMetricsInfo).sort().join(","));
console.log(
  "elu numeric:",
  ["active", "idle", "utilization"].every(
    (key) => typeof json.eventLoopUtilization[key] === "number",
  ),
);
console.log(
  "uv numeric:",
  ["events", "eventsWaiting", "loopCount"].every(
    (key) => typeof nodeTiming.uvMetricsInfo[key] === "number",
  ),
);
