import { createHook } from "node:async_hooks";

let target = -1;
let observedResource: object | undefined;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type === "PROMISE" && target === -1) {
      target = asyncId;
      observedResource = resource;
      events.push("init");
    }
  },
  promiseResolve(asyncId) {
    if (asyncId === target) events.push("resolve");
  },
}).enable();

const promise = Promise.resolve(42);
await promise;
hook.disable();
console.log("promise hook resource supplied:", observedResource === promise);
console.log("promise hook lifecycle:", events.join(">"));
