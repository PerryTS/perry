import { performance } from "node:perf_hooks";
const info: any = {
  startTime: 0,
  endTime: 1,
  encodedBodySize: 0,
  decodedBodySize: 0,
  finalConnectionTimingInfo: null,
};
for (const value of ["", "local", "invalid", null, 1]) {
  try {
    performance.markResourceTiming(
      info,
      "x",
      "fetch",
      globalThis,
      value as any,
    );
    console.log(String(value), "ok");
  } catch (error) {
    console.log(String(value), (error as Error).name, (error as any).code);
  } finally {
    performance.clearResourceTimings();
  }
}
