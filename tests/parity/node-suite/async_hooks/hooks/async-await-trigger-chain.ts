import { createHook, executionAsyncId } from "node:async_hooks";

const triggers = new Map<number, number>();
const resources = new Map<number, object>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "PROMISE") {
      triggers.set(asyncId, triggerAsyncId);
      resources.set(asyncId, resource);
    }
  },
}).enable();

let firstContinuationId = -1;
let secondContinuationId = -1;
async function exercise() {
  await null;
  firstContinuationId = executionAsyncId();
  await Promise.resolve("ready");
  secondContinuationId = executionAsyncId();
}

await exercise();
function descendsFrom(asyncId: number, ancestorId: number) {
  const visited = new Set<number>();
  let current = asyncId;
  while (current > 0 && !visited.has(current)) {
    if (current === ancestorId) return true;
    visited.add(current);
    current = triggers.get(current) ?? -1;
  }
  return false;
}

console.log(
  "async-await continuation ids:",
  firstContinuationId > 0,
  secondContinuationId > 0,
  firstContinuationId !== secondContinuationId,
);
console.log(
  "async-await resources supplied:",
  resources.has(firstContinuationId),
  resources.has(secondContinuationId),
);
console.log(
  "async-await trigger chain:",
  (triggers.get(firstContinuationId) ?? -1) > 0,
  descendsFrom(secondContinuationId, firstContinuationId),
);
hook.disable();
