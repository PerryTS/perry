import { performance, PerformanceObserver } from "node:perf_hooks";
const info: any = {
  startTime: 2,
  endTime: 5,
  encodedBodySize: 0,
  decodedBodySize: 0,
  finalConnectionTimingInfo: null,
};
let names = "not delivered";
const observer = new PerformanceObserver((list) => {
  names = list.getEntriesByType("resource").map((entry) => entry.name).join(
    ",",
  );
});
try {
  observer.observe({ entryTypes: ["resource"] });
  performance.markResourceTiming(
    info,
    "synthetic-resource",
    "fetch",
    globalThis,
    "",
  );
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("observed:", names);
} finally {
  observer.disconnect();
  performance.clearResourceTimings();
}
