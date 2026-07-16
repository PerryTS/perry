import { createHook } from "node:async_hooks";

const promiseIds = new Set<number>();
let initCount = 0;
let beforeCount = 0;
let afterCount = 0;
const hook = createHook({
  init(asyncId, type) {
    if (type === "PROMISE") {
      promiseIds.add(asyncId);
      initCount++;
    }
  },
  before(asyncId) {
    if (promiseIds.has(asyncId)) beforeCount++;
  },
  after(asyncId) {
    if (promiseIds.has(asyncId)) afterCount++;
  },
}).enable();

Promise.resolve(1).then(() => {
  hook.disable();
  Promise.resolve(42).then(() => {});
  process.nextTick(() => {});
});

setImmediate(() => {
  console.log("disable during promise init:", initCount);
  console.log("disable during promise before:", beforeCount);
  console.log("disable during promise after:", afterCount);
});
