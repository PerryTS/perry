import {
  AsyncResource,
  executionAsyncId,
  triggerAsyncId,
} from "node:async_hooks";

const parentId = executionAsyncId();
const outer = new AsyncResource("ParityOuter", { triggerAsyncId: parentId });
const inner = new AsyncResource("ParityInner", {
  triggerAsyncId: outer.asyncId(),
});

outer.runInAsyncScope(() => {
  console.log("outer execution:", executionAsyncId() === outer.asyncId());
  console.log("outer trigger:", triggerAsyncId() === parentId);

  inner.runInAsyncScope(() => {
    console.log("inner execution:", executionAsyncId() === inner.asyncId());
    console.log("inner trigger:", triggerAsyncId() === outer.asyncId());
  });

  console.log(
    "outer restored after inner:",
    executionAsyncId() === outer.asyncId(),
  );
});

console.log("parent restored after outer:", executionAsyncId() === parentId);

outer.emitDestroy();
inner.emitDestroy();
