import { performance } from "node:perf_hooks";
// mark(name, { startTime }) must reject a non-numeric startTime with a
// TypeError (Node: code ERR_INVALID_ARG_TYPE).
try {
  performance.mark("a", { startTime: "x" as any });
  console.log("threw: false");
} catch (e) {
  console.log("threw:", e instanceof TypeError);
}
