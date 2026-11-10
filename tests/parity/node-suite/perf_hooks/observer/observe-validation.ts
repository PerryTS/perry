import { PerformanceObserver } from "node:perf_hooks";

const obs = new PerformanceObserver(() => {});

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(`${label}: ok`);
  } catch (e) {
    console.log(`${label}: ${(e as Error).name}:${(e as any).code || "no-code"}`);
  }
}

outcome("missing", () => obs.observe());
outcome("null", () => obs.observe(null as any));
outcome("empty", () => obs.observe({}));
outcome("entryTypes boolean", () => obs.observe({ entryTypes: true as any }));
outcome("entryTypes with type", () =>
  obs.observe({ entryTypes: ["measure"], type: "mark" } as any)
);
outcome("empty entryTypes", () => obs.observe({ entryTypes: [] }));
outcome("bogus type", () => obs.observe({ type: "bogus" }));
obs.disconnect();
