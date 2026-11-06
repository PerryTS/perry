import { createHook, executionAsyncId } from "node:async_hooks";

let resolverExecutionId = -1;
let continuationExecutionId = -1;
const afterIds = new Set<number>();
const hook = createHook({
  after(asyncId) {
    afterIds.add(asyncId);
  },
});

const promise = new Promise<void>((resolve) => {
  setTimeout(() => {
    resolverExecutionId = executionAsyncId();
    hook.enable();
    resolve();
  }, 0);
});

await promise.then(() => {
  continuationExecutionId = executionAsyncId();
});
await new Promise<void>((resolve) => setImmediate(resolve));

console.log(
  "enabled-before-resolve continuation:",
  resolverExecutionId > 0,
  continuationExecutionId > 0,
  continuationExecutionId !== resolverExecutionId,
);
console.log(
  "enabled-before-resolve after:",
  afterIds.has(continuationExecutionId),
);
hook.disable();
