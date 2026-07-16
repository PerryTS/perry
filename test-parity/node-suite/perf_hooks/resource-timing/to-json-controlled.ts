import { performance } from "node:perf_hooks";
performance.clearResourceTimings();
const timing: any = {
  startTime: 1,
  endTime: 4,
  encodedBodySize: 5,
  decodedBodySize: 6,
  finalConnectionTimingInfo: null,
};
try {
  const json: any = performance.markResourceTiming(
    timing,
    "controlled",
    "other",
    globalThis,
    "",
    {},
    204,
    "direct",
  ).toJSON();
  console.log("keys:", Object.keys(json).sort().join(","));
  console.log(
    "stable:",
    json.name,
    json.entryType,
    json.startTime,
    json.duration,
    json.transferSize,
    json.encodedBodySize,
    json.decodedBodySize,
    json.responseStatus,
    json.deliveryType,
  );
} finally {
  performance.clearResourceTimings();
}
