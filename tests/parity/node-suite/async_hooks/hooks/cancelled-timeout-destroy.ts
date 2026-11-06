import { createHook } from "node:async_hooks";

let target = -1;
let observedResource: object | undefined;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type === "Timeout" && target === -1) {
      target = asyncId;
      observedResource = resource;
      events.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === target) events.push("before");
  },
  after(asyncId) {
    if (asyncId === target) events.push("after");
  },
  destroy(asyncId) {
    if (asyncId === target) events.push("destroy");
  },
}).enable();

const timeout = setTimeout(() => events.push("callback"), 1000);
clearTimeout(timeout);
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log(
  "cancelled timeout resource identity:",
  observedResource === timeout,
);
console.log("cancelled timeout lifecycle:", events.join(">"));
