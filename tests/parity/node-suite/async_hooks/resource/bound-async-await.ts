import { AsyncResource, executionAsyncId } from "node:async_hooks";

const parentId = executionAsyncId();
const resource = new AsyncResource("ParityBoundAsync");

const bound = resource.bind(async (value: string) => {
  console.log("bound async start:", executionAsyncId() === resource.asyncId());
  await Promise.resolve();
  console.log(
    "bound async continuation:",
    executionAsyncId() === resource.asyncId(),
  );
  return value.toUpperCase();
});

const pending = bound("value");
console.log("bound async immediate restore:", executionAsyncId() === parentId);
console.log("bound async result:", await pending);
console.log("bound async final restore:", executionAsyncId() === parentId);

resource.emitDestroy();
