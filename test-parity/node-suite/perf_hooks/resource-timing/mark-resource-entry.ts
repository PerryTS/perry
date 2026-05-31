import { performance } from "node:perf_hooks";

performance.clearResourceTimings();

const before = performance.getEntriesByType("resource").length;
const timingInfo: any = {
  startTime: 10,
  redirectStart: 0,
  redirectEnd: 0,
  fetchStart: 10,
  domainLookupStart: 11,
  domainLookupEnd: 12,
  connectStart: 13,
  connectEnd: 14,
  requestStart: 15,
  responseStart: 16,
  responseEnd: 20,
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
console.log("fields:", last.name, last.entryType, last.initiatorType, last.startTime);
console.log("duration nan:", Number.isNaN(last.duration));
performance.clearResourceTimings();
console.log("after clear:", performance.getEntriesByType("resource").length);
