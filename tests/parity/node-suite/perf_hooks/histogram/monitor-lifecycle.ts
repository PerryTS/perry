import { monitorEventLoopDelay } from "node:perf_hooks";
const h = monitorEventLoopDelay();
try {
  console.log("enable:", h.enable(), h.enable());
  console.log("disable:", h.disable(), h.disable());
  console.log("reenable:", h.enable());
} finally {
  h.disable();
}
