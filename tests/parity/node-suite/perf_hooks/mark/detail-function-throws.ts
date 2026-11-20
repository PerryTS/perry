import { performance } from "node:perf_hooks";
// mark detail with a Function value is not structured-cloneable and throws
// DataCloneError.
let threw = false;
try {
  performance.mark("fn", { detail: () => {} });
} catch {
  threw = true;
}
console.log("threw:", threw);
