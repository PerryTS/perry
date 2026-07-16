import { createHook, executionAsyncId } from "node:async_hooks";

let target = -1;
let triggerMatches = false;
const root = executionAsyncId();
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "TickObject" && target === -1) {
      target = asyncId;
      triggerMatches = triggerAsyncId === root;
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

await new Promise<void>((resolve) => process.nextTick(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("nextTick hook trigger root:", triggerMatches);
console.log("nextTick hook lifecycle:", events.join(">"));
