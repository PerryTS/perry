import {
  AsyncResource,
  executionAsyncId,
  triggerAsyncId,
} from "node:async_hooks";

const outerId = executionAsyncId();
const resource = new AsyncResource("ParityScope", { triggerAsyncId: outerId });
const receiver = { label: "bound" };

const result = resource.runInAsyncScope(
  function (this: typeof receiver, left: number, right: number) {
    console.log(
      "scope execution id:",
      executionAsyncId() === resource.asyncId(),
    );
    console.log("scope trigger id:", triggerAsyncId() === outerId);
    console.log("scope receiver:", this === receiver);
    console.log("scope args:", left, right);
    return left + right;
  },
  receiver,
  2,
  5,
);

console.log("scope return:", result);
console.log("scope restores execution:", executionAsyncId() === outerId);

const bound = resource.bind(function (
  this: typeof receiver,
  prefix: string,
  value: number,
) {
  console.log("bind execution id:", executionAsyncId() === resource.asyncId());
  console.log("bind receiver:", this === receiver);
  console.log("bind args:", prefix, value);
  return `${prefix}:${value}`;
}, receiver);

console.log("bound shape:", typeof bound, bound.length);
console.log("bound return:", bound("value", 9));
console.log("bind restores execution:", executionAsyncId() === outerId);

resource.emitDestroy();
