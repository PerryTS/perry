import { createHook } from "node:async_hooks";

let defaultResource: object | undefined;
const defaultHook = createHook({
  init(_asyncId, type, _triggerAsyncId, resource) {
    if (type === "PROMISE") defaultResource = resource;
  },
}).enable();
const defaultPromise = Promise.resolve("default");
defaultHook.disable();
console.log(
  "trackPromises default resource:",
  defaultResource === defaultPromise,
);

let explicitCount = 0;
const explicitHook = createHook({
  init(_asyncId, type) {
    if (type === "PROMISE") explicitCount++;
  },
  trackPromises: true,
}).enable();
Promise.resolve("explicit");
explicitHook.disable();
console.log("trackPromises true init count:", explicitCount);

let disabledCount = 0;
const disabledHook = createHook({
  init(_asyncId, type) {
    if (type === "PROMISE") disabledCount++;
  },
  trackPromises: false,
}).enable();
Promise.resolve("disabled");
disabledHook.disable();
console.log("trackPromises false init count:", disabledCount);
