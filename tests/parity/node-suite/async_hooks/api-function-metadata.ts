import {
  AsyncLocalStorage,
  AsyncResource,
  createHook,
  executionAsyncId,
  executionAsyncResource,
  triggerAsyncId,
} from "node:async_hooks";

function metadata(entries: Array<[string, unknown]>) {
  return entries
    .map(([label, value]) =>
      typeof value === "function"
        ? `${label}:${value.name}/${value.length}`
        : `${label}:missing`,
    )
    .join("|");
}

console.log(
  "module function metadata:",
  metadata([
    ["createHook", createHook],
    ["executionAsyncId", executionAsyncId],
    ["executionAsyncResource", executionAsyncResource],
    ["triggerAsyncId", triggerAsyncId],
  ]),
);
console.log(
  "storage function metadata:",
  metadata([
    ["constructor", AsyncLocalStorage],
    ["bind", AsyncLocalStorage.bind],
    ["snapshot", AsyncLocalStorage.snapshot],
    ["run", AsyncLocalStorage.prototype.run],
    ["getStore", AsyncLocalStorage.prototype.getStore],
    ["enterWith", AsyncLocalStorage.prototype.enterWith],
    ["exit", AsyncLocalStorage.prototype.exit],
    ["disable", AsyncLocalStorage.prototype.disable],
  ]),
);
console.log(
  "resource function metadata:",
  metadata([
    ["constructor", AsyncResource],
    ["staticBind", AsyncResource.bind],
    ["asyncId", AsyncResource.prototype.asyncId],
    ["triggerAsyncId", AsyncResource.prototype.triggerAsyncId],
    ["emitDestroy", AsyncResource.prototype.emitDestroy],
    ["runInAsyncScope", AsyncResource.prototype.runInAsyncScope],
    ["bind", AsyncResource.prototype.bind],
  ]),
);
