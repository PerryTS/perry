import { monitorEventLoopDelay } from "node:perf_hooks";
const first = monitorEventLoopDelay();
const second = monitorEventLoopDelay();
try {
  console.log("distinct:", first !== second);
  console.log("constructors:", first.constructor.name, second.constructor.name);
} finally {
  first.disable();
  second.disable();
}
