import { AsyncResource, executionAsyncId } from "node:async_hooks";

const parentId = executionAsyncId();
const resource = new AsyncResource("ParityAsyncScope");

const pending = resource.runInAsyncScope(async () => {
  console.log("async scope start:", executionAsyncId() === resource.asyncId());
  await Promise.resolve();
  console.log(
    "async scope continuation:",
    executionAsyncId() === resource.asyncId(),
  );
  return "async-result";
});

console.log("async scope immediate restore:", executionAsyncId() === parentId);
console.log("async scope result:", await pending);
console.log("async scope final restore:", executionAsyncId() === parentId);

resource.emitDestroy();
