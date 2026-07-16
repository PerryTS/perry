import { performance, PerformanceObserver } from "node:perf_hooks";
let checked = false;
const observer = new PerformanceObserver((list) => {
  const first = list.getEntries();
  const second = list.getEntries();
  first.length = 0;
  console.log("fresh:", first !== second);
  console.log("same entry:", second[0] === list.getEntries()[0]);
  console.log("isolated:", second.length);
  checked = true;
});
try {
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("snapshot");
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("callback:", checked);
} finally {
  observer.disconnect();
  performance.clearMarks();
}
