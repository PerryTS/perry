import { performance } from "node:perf_hooks";

performance.clearResourceTimings();

const before = performance.getEntriesByType("resource").length;
const timingInfo: any = {
  startTime: 10,
  redirectStartTime: 0,
  redirectEndTime: 0,
  postRedirectStartTime: 10,
  finalServiceWorkerStartTime: 0,
  finalNetworkRequestStartTime: 15,
  finalNetworkResponseStartTime: 16,
  endTime: 20,
  encodedBodySize: 0,
  decodedBodySize: 0,
  finalConnectionTimingInfo: null,
};

const returned: any = performance.markResourceTiming(
  timingInfo,
  "https://example.test/a",
  "fetch",
  globalThis,
  "",
);
const entries: any[] = performance.getEntriesByType("resource") as any[];
const last: any = entries[entries.length - 1];

console.log("returned object:", typeof returned);
console.log("same returned:", returned === last);
console.log("delta:", entries.length - before);
console.log(
  "fields:",
  last.name,
  last.entryType,
  last.initiatorType,
  last.startTime,
);
console.log("duration:", last.duration);
performance.clearResourceTimings();
console.log("after clear:", performance.getEntriesByType("resource").length);
