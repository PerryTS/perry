// The runtime unit test covers the first empty checkpoint directly. This
// fixture checks the public perf_hooks surface after the loop has drained.
import { performance } from "node:perf_hooks";
process.on("beforeExit", () => {
  console.log("loop started", performance.nodeTiming.loopStart >= 0);
});
