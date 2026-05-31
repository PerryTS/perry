import { performance } from "node:perf_hooks";

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(`${label}: ok`);
  } catch (e) {
    console.log(`${label}: ${(e as Error).name}:${(e as any).code || "no-code"}`);
  }
}

outcome("all endpoints", () =>
  performance.measure("all", { start: 1, end: 2, duration: 3 } as any)
);
outcome("missing start mark", () =>
  performance.measure("missing start", { start: "missing", end: 10 })
);
outcome("missing end mark", () =>
  performance.measure("missing end", { start: 0, end: "missing" })
);
outcome("negative duration", () =>
  performance.measure("negative duration", { start: 10, duration: -5 })
);

const ok = performance.measure("valid", { start: 5, duration: 10 });
console.log("valid start:", ok.startTime);
console.log("valid duration:", ok.duration);
