import { createHook, executionAsyncId } from "node:async_hooks";

let firstExecutionId = -1;
let secondExecutionId = -1;
const beforeIds = new Set<number>();
const afterIds = new Set<number>();
let initCount = 0;

const parent = Promise.resolve(1);
const first = parent.then((value) => {
  firstExecutionId = executionAsyncId();
  return value + 1;
});
first.then((value) => {
  secondExecutionId = executionAsyncId();
  return value + 1;
});
const barrier = setTimeout(() => {
  hook.disable();
  console.log("precreated promise init count:", initCount);
  console.log(
    "precreated promise ids valid:",
    firstExecutionId > 0,
    secondExecutionId > 0,
    firstExecutionId !== secondExecutionId,
  );
  console.log(
    "precreated promise before delivery:",
    beforeIds.has(firstExecutionId),
    beforeIds.has(secondExecutionId),
  );
  console.log(
    "precreated promise after delivery:",
    afterIds.has(firstExecutionId),
    afterIds.has(secondExecutionId),
  );
}, 0);

const hook = createHook({
  init() {
    initCount++;
  },
  before(asyncId) {
    beforeIds.add(asyncId);
  },
  after(asyncId) {
    afterIds.add(asyncId);
  },
}).enable();
void barrier;
