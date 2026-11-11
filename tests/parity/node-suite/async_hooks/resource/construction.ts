import { AsyncResource, createHook, executionAsyncId } from "node:async_hooks";

const observed: Array<{
  asyncId: number;
  type: string;
  triggerAsyncId: number;
}> = [];

const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "ParityConstruction") {
      observed.push({ asyncId, type, triggerAsyncId });
    }
  },
}).enable();

const parentId = executionAsyncId();
const resource = new AsyncResource("ParityConstruction");
const explicit = new AsyncResource("ExplicitTrigger", { triggerAsyncId: 73 });

console.log("custom init count:", observed.length);
console.log("custom type:", observed[0]?.type);
console.log("async id positive:", resource.asyncId() > 0);
console.log(
  "async id matches init:",
  resource.asyncId() === observed[0]?.asyncId,
);
console.log(
  "default trigger matches parent:",
  resource.triggerAsyncId() === parentId,
);
console.log(
  "init trigger matches resource:",
  observed[0]?.triggerAsyncId === resource.triggerAsyncId(),
);
console.log(
  "ids are distinct:",
  resource.asyncId() !== resource.triggerAsyncId(),
);
console.log("explicit trigger:", explicit.triggerAsyncId());

hook.disable();
resource.emitDestroy();
explicit.emitDestroy();
