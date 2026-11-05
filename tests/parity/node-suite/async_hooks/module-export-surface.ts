import asyncHooksDefault from "node:async_hooks";
import * as asyncHooks from "node:async_hooks";

const expected = [
  "AsyncLocalStorage",
  "AsyncResource",
  "asyncWrapProviders",
  "createHook",
  "default",
  "executionAsyncId",
  "executionAsyncResource",
  "triggerAsyncId",
];
const keys = Object.keys(asyncHooks);
console.log("module export keys:", keys.join(","));
console.log(
  "module export set:",
  keys.length === expected.length &&
    [...keys].sort().join(",") === [...expected].sort().join(","),
);
console.log(
  "module export descriptors:",
  expected.every((name) => {
    const descriptor = Object.getOwnPropertyDescriptor(asyncHooks, name);
    return (
      !!descriptor &&
      descriptor.enumerable === true &&
      descriptor.configurable === false &&
      descriptor.writable === true
    );
  }),
);
const originalCreateHook = asyncHooks.createHook;
console.log(
  "module namespace immutable:",
  Reflect.set(asyncHooks, "createHook", null),
  asyncHooks.createHook === originalCreateHook,
);
console.log(
  "default export keys:",
  Object.keys(asyncHooksDefault).sort().join(","),
);
