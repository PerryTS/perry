import { performance, PerformanceObserver } from "node:perf_hooks";

function check(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + " :: NO THROW");
  } catch (err: any) {
    console.log(label + " :: " + err.name + ": " + err.message);
  }
}

// #3088 — performance.measure(name) requires a string name.
console.log("== 3088 measure name validation ==");
check("measure(undefined)", () => performance.measure(undefined as any));
check("measure(null)", () => performance.measure(null as any));
check("measure(0)", () => performance.measure(0 as any));
check("measure(true)", () => performance.measure(true as any));
check("measure({})", () => performance.measure({} as any));
check("measure([])", () => performance.measure([] as any));
check("measure(Symbol)", () => performance.measure(Symbol("s") as any));
// control: mark() still string-coerces.
console.log("mark(1).name == " + performance.mark(1 as any).name);

// #3008 — option endpoint + timestamp validation.
console.log("== 3008 option/timestamp validation ==");
check("mark neg startTime", () => performance.mark("neg", { startTime: -1 }));
check("mark startTime non-number", () =>
  performance.mark("nn", { startTime: "x" as any }));
check("measure start+end+duration", () =>
  performance.measure("all", { start: 1, end: 2, duration: 3 }));
check("measure start missing-mark", () =>
  performance.measure("u1", { start: "missing", end: 10 }));
check("measure end missing-mark", () =>
  performance.measure("u2", { start: 0, end: "missing" }));
check("measure negative duration", () =>
  performance.measure("negDur", { start: 10, duration: -5 }));
// controls: valid option combinations still succeed.
check("measure {start,end}", () => performance.measure("ok1", { start: 0, end: 5 }));
check("measure {start,duration}", () =>
  performance.measure("ok2", { start: 0, duration: 5 }));
check("measure {duration,end}", () =>
  performance.measure("ok3", { duration: 5, end: 10 }));

// #3010 — PerformanceObserver.observe option validation.
console.log("== 3010 observe validation ==");
const obs = new PerformanceObserver(() => {});
check("observe()", () => obs.observe());
check("observe(null)", () => obs.observe(null as any));
check("observe({})", () => obs.observe({} as any));
check("observe entryTypes string", () =>
  obs.observe({ entryTypes: "mark" as any }));
check("observe type+entryTypes", () =>
  obs.observe({ type: "mark", entryTypes: ["measure"] } as any));
check("observe entryTypes []", () => obs.observe({ entryTypes: [] }));
check("observe entryTypes bogus", () => obs.observe({ entryTypes: ["bogus"] }));
check("observe type bogus", () => obs.observe({ type: "bogus" }));

// #3011 — two-argument eventLoopUtilization diffs. Assert SHAPE/keys, never
// raw timestamps (those vary run-to-run).
console.log("== 3011 eventLoopUtilization diffs ==");
const a = performance.eventLoopUtilization();
const b = performance.eventLoopUtilization();
const twoArg = performance.eventLoopUtilization(b, a);
const oneArg = performance.eventLoopUtilization(b);
console.log("twoArg keys: " + Object.keys(twoArg).sort().join(","));
console.log("oneArg keys: " + Object.keys(oneArg).sort().join(","));
console.log(
  "twoArg shape: idle>=0=" +
    (twoArg.idle >= 0) +
    " active>=0=" +
    (twoArg.active >= 0) +
    " util-in-range=" +
    (twoArg.utilization >= 0 && twoArg.utilization <= 1),
);
console.log(
  "oneArg shape: idle>=0=" +
    (oneArg.idle >= 0) +
    " active>=0=" +
    (oneArg.active >= 0) +
    " util-in-range=" +
    (oneArg.utilization >= 0 && oneArg.utilization <= 1),
);
console.log(
  "types: " +
    typeof twoArg.idle +
    "," +
    typeof twoArg.active +
    "," +
    typeof twoArg.utilization,
);
