import { createHook } from "node:async_hooks";

let target = -1;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type) {
    if (type === "Timeout" && target === -1) {
      target = asyncId;
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
console.log("cancelled timeout lifecycle:", events.join(">"));
