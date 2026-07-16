import { monitorEventLoopDelay } from "node:perf_hooks";
const h = monitorEventLoopDelay();
try {
  console.log("surface:", typeof h.reset, typeof h.enable, typeof h.disable);
  console.log("initial:", h.count, h.countBigInt === 0n);
  console.log("reset:", h.reset() === undefined, h.count);
  console.log(
    "handle methods:",
    typeof (h as any).ref,
    typeof (h as any).unref,
    typeof (h as any).hasRef,
  );
} finally {
  h.disable();
}
