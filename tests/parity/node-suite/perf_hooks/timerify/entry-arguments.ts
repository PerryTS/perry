import { PerformanceObserver, timerify } from "node:perf_hooks";
const argument = { value: 2 };
let result = "not delivered";
const observer = new PerformanceObserver((list) => {
  const entry: any = list.getEntries()[0];
  result = `${entry.detail.length}:${entry.detail[0]}:${
    entry.detail[1] === argument
  }:${entry[0]}:${entry[1] === argument}`;
});
try {
  observer.observe({ entryTypes: ["function"] });
  timerify(function sample(_a: number, _b: object) {})(1, argument);
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("arguments:", result);
} finally {
  observer.disconnect();
}
