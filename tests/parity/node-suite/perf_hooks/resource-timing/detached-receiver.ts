import { performance } from "node:perf_hooks";

// undici reads `performance.markResourceTiming` once at module scope and calls
// it detached for every fetch (#13964). Node does not brand-check it, unlike
// the other Performance methods.
performance.clearResourceTimings();

const timingInfo: any = {
  startTime: 0,
  redirectStartTime: 0,
  redirectEndTime: 0,
  postRedirectStartTime: 0,
  finalServiceWorkerStartTime: 0,
  finalNetworkResponseStartTime: 0,
  finalNetworkRequestStartTime: 0,
  endTime: 1,
  encodedBodySize: 0,
  decodedBodySize: 0,
  finalConnectionTimingInfo: null,
};

function args(url: string, status: number): any[] {
  return [timingInfo, url, "fetch", globalThis, "", {}, status, ""];
}

function outcome(label: string, fn: () => any) {
  try {
    const entry = fn();
    console.log(label, "ok", entry.name, entry.entryType, entry.responseStatus);
  } catch (error) {
    console.log(label, (error as Error).name, (error as any).code);
  }
}

const markResourceTiming: any = performance.markResourceTiming;
outcome("detached", () => markResourceTiming(...args("http://x/a", 200)));
outcome(
  "foreign receiver",
  () =>
    Reflect.apply(
      Object.getPrototypeOf(performance).markResourceTiming,
      {},
      args("http://x/b", 201),
    ),
);
outcome(
  "undefined receiver",
  () => Reflect.apply(markResourceTiming, undefined, args("http://x/c", 202)),
);

console.log(
  "names:",
  performance.getEntriesByType("resource").map((e) => e.name).join(","),
);

// The rest of Performance.prototype keeps its brand check.
const now: any = performance.now;
try {
  now();
  console.log("detached now: no throw");
} catch (error) {
  console.log("detached now:", (error as Error).name, (error as any).code);
}

performance.clearResourceTimings();
console.log("after clear:", performance.getEntriesByType("resource").length);
