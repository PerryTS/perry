import { performance } from "node:perf_hooks";
const info: any = {
  startTime: 0,
  endTime: 1,
  encodedBodySize: 150,
  decodedBodySize: 250,
  finalConnectionTimingInfo: null,
};
performance.clearResourceTimings();
try {
  const network: any = performance.markResourceTiming(
    info,
    "network",
    "fetch",
    globalThis,
    "",
  );
  const local: any = performance.markResourceTiming(
    info,
    "local",
    "fetch",
    globalThis,
    "local",
  );
  console.log("network:", network.transferSize);
  console.log("local:", local.transferSize);
} finally {
  performance.clearResourceTimings();
}
