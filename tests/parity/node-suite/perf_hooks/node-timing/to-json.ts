import { performance } from "node:perf_hooks";
const timing = performance.nodeTiming;
const json = timing.toJSON();
console.log("keys:", Object.keys(json).sort().join(","));
console.log(
  "stable:",
  json.name,
  json.entryType,
  json.startTime,
  json.nodeStart === timing.nodeStart,
);
console.log("fresh:", json !== timing.toJSON());
console.log(
  "numeric:",
  [
    "duration",
    "nodeStart",
    "v8Start",
    "environment",
    "bootstrapComplete",
    "loopStart",
    "loopExit",
    "idleTime",
  ].every((key) => typeof json[key] === "number"),
);
