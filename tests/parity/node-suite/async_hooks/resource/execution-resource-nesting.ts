import { AsyncResource, executionAsyncResource } from "node:async_hooks";

const topResource = executionAsyncResource();
const outer = new AsyncResource("ParityExecutionOuter");
const inner = new AsyncResource("ParityExecutionInner");

outer.runInAsyncScope(() => {
  console.log("outer resource identity:", executionAsyncResource() === outer);

  inner.runInAsyncScope(() => {
    console.log("inner resource identity:", executionAsyncResource() === inner);
  });

  console.log("outer resource restored:", executionAsyncResource() === outer);
});

console.log("top resource restored:", executionAsyncResource() === topResource);

outer.emitDestroy();
inner.emitDestroy();
