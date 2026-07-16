import { createHook, executionAsyncId } from "node:async_hooks";

let targetId = -1;
let afterSeen = false;
let destroySeen = false;
const hook = createHook({
  after(asyncId) {
    if (asyncId === targetId) afterSeen = true;
  },
  destroy(asyncId) {
    if (asyncId === targetId) destroySeen = true;
  },
});

process.nextTick(() => {
  targetId = executionAsyncId();
  hook.enable();
  setImmediate(() =>
    setImmediate(() => {
      console.log("late nextTick after/destroy:", afterSeen, destroySeen);
      hook.disable();
    }),
  );
});
