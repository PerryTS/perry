import { createHook, executionAsyncId, triggerAsyncId } from "node:async_hooks";

let outerId = -1;
const children: Array<{ asyncId: number; triggerAsyncId: number }> = [];
const callbackChecks: boolean[] = [];
const hook = createHook({
  init(asyncId, type, parentId) {
    if (type !== "TickObject") return;
    if (outerId === -1) {
      outerId = asyncId;
    } else if (parentId === outerId) {
      children.push({ asyncId, triggerAsyncId: parentId });
    }
  },
}).enable();

await new Promise<void>((resolve) => {
  process.nextTick(() => {
    let completed = 0;
    for (let index = 0; index < 2; index++) {
      process.nextTick(() => {
        const child = children[index];
        callbackChecks.push(
          !!child &&
            executionAsyncId() === child.asyncId &&
            triggerAsyncId() === outerId,
        );
        if (++completed === 2) resolve();
      });
    }
  });
});
hook.disable();

console.log("nextTick sibling count:", children.length);
console.log(
  "nextTick sibling relationships:",
  children.length === 2 &&
    children.every((child) => child.triggerAsyncId === outerId),
  children.length === 2 && children[0].asyncId !== children[1].asyncId,
  callbackChecks.join(","),
);
