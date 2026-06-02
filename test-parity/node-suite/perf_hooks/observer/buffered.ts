import { performance, PerformanceObserver } from "node:perf_hooks";
// observe({ type, buffered: true }) delivers entries that already existed
// before observe() was called. (Fallback timer guards against a non-delivering
// runtime so the test reports 0 instead of hanging.)
performance.mark("pre1");
performance.mark("pre2");
await new Promise<void>((resolve) => {
  let done = false;
  const finish = (n: number) => {
    if (done) return;
    done = true;
    console.log("buffered count:", n);
    resolve();
  };
  const obs = new PerformanceObserver((list) => {
    obs.disconnect();
    finish(list.getEntries().length);
  });
  obs.observe({ type: "mark", buffered: true });
  setTimeout(() => finish(0), 300);
});
