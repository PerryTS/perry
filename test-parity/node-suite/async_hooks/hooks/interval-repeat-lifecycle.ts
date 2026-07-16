import { createHook, executionAsyncId } from "node:async_hooks";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
let targetResource: object | undefined;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "Timeout" && targetId === -1) {
      targetId = asyncId;
      targetTriggerId = triggerAsyncId;
      targetResource = resource;
      events.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === targetId) events.push("before");
  },
  after(asyncId) {
    if (asyncId === targetId) events.push("after");
  },
  destroy(asyncId) {
    if (asyncId === targetId) events.push("destroy");
  },
}).enable();

let interval: ReturnType<typeof setInterval>;
let calls = 0;
await new Promise<void>((resolve) => {
  interval = setInterval(() => {
    calls++;
    if (calls === 2) {
      clearInterval(interval);
      resolve();
    }
  }, 1);
});
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log(
  "interval resource relationship:",
  targetId > 0,
  targetTriggerId === parentId,
  targetResource === interval,
);
console.log("interval repeated calls:", calls);
console.log("interval repeated lifecycle:", events.join(">"));
