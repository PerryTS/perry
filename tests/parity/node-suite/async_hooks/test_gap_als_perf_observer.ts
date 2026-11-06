// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

import { PerformanceObserver, performance } from "node:perf_hooks";
als.run("observer", () => {
  const o = new PerformanceObserver((list) => {
    console.log(list.getEntries()[0].name + ": " + als.getStore());
    o.disconnect();
  });
  o.observe({ entryTypes: ["mark"] });
});
als.run("mark", () => performance.mark("m"));
