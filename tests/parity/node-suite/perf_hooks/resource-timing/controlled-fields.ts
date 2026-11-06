import {
  performance,
  PerformanceEntry,
  PerformanceResourceTiming,
} from "node:perf_hooks";
performance.clearResourceTimings();
const timing: any = {
  startTime: 10,
  redirectStartTime: 11,
  redirectEndTime: 12,
  postRedirectStartTime: 13,
  finalServiceWorkerStartTime: 14,
  finalNetworkRequestStartTime: 20,
  finalNetworkResponseStartTime: 30,
  endTime: 40,
  encodedBodySize: 150,
  decodedBodySize: 250,
  finalConnectionTimingInfo: {
    domainLookupStartTime: 15,
    domainLookupEndTime: 16,
    connectionStartTime: 17,
    connectionEndTime: 18,
    secureConnectionStartTime: 19,
    ALPNNegotiatedProtocol: "h2",
  },
};
try {
  const entry: any = performance.markResourceTiming(
    timing,
    "https://example.test/resource",
    "fetch",
    globalThis,
    "",
    {},
    201,
    "cache",
  );
  console.log(
    "instance:",
    entry instanceof PerformanceEntry,
    entry instanceof PerformanceResourceTiming,
  );
  console.log(
    "base:",
    entry.name,
    entry.entryType,
    entry.initiatorType,
    entry.startTime,
    entry.duration,
  );
  console.log(
    "network:",
    entry.domainLookupStart,
    entry.domainLookupEnd,
    entry.connectStart,
    entry.connectEnd,
    entry.secureConnectionStart,
    entry.nextHopProtocol,
  );
  console.log(
    "response:",
    entry.requestStart,
    entry.responseStart,
    entry.responseEnd,
    entry.encodedBodySize,
    entry.decodedBodySize,
    entry.transferSize,
    entry.responseStatus,
    entry.deliveryType,
  );
} finally {
  performance.clearResourceTimings();
}
