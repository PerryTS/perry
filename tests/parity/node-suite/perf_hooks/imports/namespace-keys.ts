import * as hooks from "node:perf_hooks";
console.log(Object.keys(hooks).sort().join(","));
console.log(
  "default identity:",
  hooks.default.performance === hooks.performance,
);
