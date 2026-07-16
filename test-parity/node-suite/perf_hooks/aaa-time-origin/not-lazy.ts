import { performance } from "node:perf_hooks";

const before = performance.now();
await new Promise<void>((resolve) => setImmediate(resolve));
const after = performance.now();
console.log(
  "timeOrigin stable:",
  performance.timeOrigin === performance.timeOrigin,
);
console.log("now advances:", after >= before);
console.log(
  "origin relation:",
  performance.timeOrigin + after >= performance.timeOrigin,
);
