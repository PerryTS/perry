import { createHook } from "node:async_hooks";

let target = -1;
let observedResource: object | undefined;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type === "Immediate" && target === -1) {
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

const immediate = setImmediate(() => events.push("callback"));
clearImmediate(immediate);
await new Promise<void>((resolve) => setTimeout(resolve, 0));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log(
  "cancelled immediate resource identity:",
  observedResource === immediate,
);
console.log("cancelled immediate lifecycle:", events.join(">"));
