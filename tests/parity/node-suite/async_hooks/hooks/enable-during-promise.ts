import { createHook } from "node:async_hooks";

Promise.resolve().then(() => {
  const tickIds = new Set<number>();
  let initCount = 0;
  let beforeCount = 0;
  let afterCount = 0;
  const hook = createHook({
    init(asyncId, type) {
      if (type === "TickObject") {
        tickIds.add(asyncId);
        initCount++;
      }
    },
    before(asyncId) {
      if (tickIds.has(asyncId)) beforeCount++;
    },
    after(asyncId) {
      if (tickIds.has(asyncId)) afterCount++;
    },
  }).enable();

  process.nextTick(() => {});
  setImmediate(() => {
    hook.disable();
    console.log("enable during promise tick init:", initCount);
    console.log("enable during promise tick before:", beforeCount);
    console.log("enable during promise tick after:", afterCount);
  });
});
