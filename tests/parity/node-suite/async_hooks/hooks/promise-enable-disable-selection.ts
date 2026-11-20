import { createHook } from "node:async_hooks";

const observed = new WeakSet<object>();
const hook = createHook({
  init(_asyncId, type, _triggerAsyncId, resource) {
    if (type === "PROMISE") observed.add(resource);
  },
});

hook.disable();
const disabledFirst = Promise.resolve("disabled-first");
hook.enable();
const enabledFirst = Promise.resolve("enabled-first");
hook.disable();
const disabledSecond = Promise.resolve("disabled-second");
hook.enable();
const enabledSecond = Promise.resolve("enabled-second");
hook.disable();

await Promise.all([disabledFirst, enabledFirst, disabledSecond, enabledSecond]);
console.log(
  "promise enable selection:",
  observed.has(disabledFirst),
  observed.has(enabledFirst),
  observed.has(disabledSecond),
  observed.has(enabledSecond),
);
