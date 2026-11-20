import { performance, PerformanceObserver } from "node:perf_hooks";

const seen: string[] = [];
const obs = new PerformanceObserver((list: any) => {
  for (const entry of list.getEntries()) {
    seen.push(
      `${entry.name}:${entry.entryType}:${typeof entry.duration}:${
        entry.duration >= 0
      }`,
    );
  }
});

obs.observe({ entryTypes: ["function"] });

function add(a: number, b: number): number {
  return a + b;
}

const wrapped = performance.timerify(add);
console.log("same fn:", wrapped === add);
console.log("name:", wrapped.name);
console.log("length:", wrapped.length);
console.log("result:", wrapped(2, 3));

try {
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("seen count:", seen.length);
  console.log("first:", seen[0] || "none");
} finally {
  obs.disconnect();
}
