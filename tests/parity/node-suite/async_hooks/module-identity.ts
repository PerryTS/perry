import asyncHooksDefault from "async_hooks";
import * as bare from "async_hooks";
import * as prefixed from "node:async_hooks";

const names = [
  "AsyncLocalStorage",
  "AsyncResource",
  "createHook",
  "executionAsyncId",
  "executionAsyncResource",
  "triggerAsyncId",
  "asyncWrapProviders",
] as const;
console.log(
  "specifier identities:",
  names.map((name) => bare[name] === prefixed[name]).join(","),
);
console.log(
  "default identities:",
  names.map((name) => asyncHooksDefault[name] === prefixed[name]).join(","),
);
