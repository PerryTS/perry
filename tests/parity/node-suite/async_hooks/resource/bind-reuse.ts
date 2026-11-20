import {
  AsyncResource,
  executionAsyncId,
  triggerAsyncId,
} from "node:async_hooks";

const parentId = executionAsyncId();
const resource = new AsyncResource("ParityBindReuse", {
  triggerAsyncId: parentId,
});

let calls = 0;
const bound = resource.bind((value: string) => {
  calls++;
  console.log("reuse call:", calls, value);
  console.log("reuse execution:", executionAsyncId() === resource.asyncId());
  console.log("reuse trigger:", triggerAsyncId() === parentId);
  return `${value}-${calls}`;
});

console.log("first reuse result:", bound("first"));
console.log("first reuse restored:", executionAsyncId() === parentId);
console.log("second reuse result:", bound("second"));
console.log("second reuse restored:", executionAsyncId() === parentId);

resource.emitDestroy();
