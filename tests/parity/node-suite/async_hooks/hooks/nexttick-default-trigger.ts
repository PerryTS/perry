import { createHook, executionAsyncId, triggerAsyncId } from "node:async_hooks";

const parentId = executionAsyncId();
let tickId = -1;
let initTrigger = -1;
let beforeCount = 0;
let afterCount = 0;
let destroyCount = 0;
const hook = createHook({
  init(asyncId, type, triggerId) {
    if (type === "TickObject" && tickId === -1) {
      tickId = asyncId;
      initTrigger = triggerId;
    }
  },
  before(asyncId) {
    if (asyncId === tickId) beforeCount++;
  },
  after(asyncId) {
    if (asyncId === tickId) afterCount++;
  },
  destroy(asyncId) {
    if (asyncId === tickId) destroyCount++;
  },
}).enable();

process.nextTick(() => {
  console.log(
    "nextTick trigger relationships:",
    tickId > 0,
    initTrigger === parentId,
    triggerAsyncId() === parentId,
    executionAsyncId() === tickId,
  );
});

setImmediate(() =>
  setImmediate(() => {
    console.log("nextTick lifecycle:", beforeCount, afterCount, destroyCount);
    hook.disable();
  }),
);
