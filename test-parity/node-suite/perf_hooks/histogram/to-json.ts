import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
h.record(3);
h.record(7);
const json = h.toJSON();
console.log("keys:", Object.keys(json).sort().join(","));
console.log("stable fields:", json.count, json.min, json.max, json.exceeds);
console.log(
  "percentiles object:",
  typeof json.percentiles === "object" && json.percentiles !== null,
);
