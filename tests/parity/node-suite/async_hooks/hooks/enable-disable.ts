import { AsyncResource, createHook } from "node:async_hooks";

let initCount = 0;
const hook = createHook({
  init(asyncId, type) {
    if (type === "ParityEnableDisable") initCount++;
  },
});

console.log("first enable identity:", hook.enable() === hook);
console.log("second enable identity:", hook.enable() === hook);
new AsyncResource("ParityEnableDisable").emitDestroy();
console.log("enabled init count:", initCount);

console.log("first disable identity:", hook.disable() === hook);
console.log("second disable identity:", hook.disable() === hook);
new AsyncResource("ParityEnableDisable").emitDestroy();
console.log("disabled init count:", initCount);

hook.enable();
new AsyncResource("ParityEnableDisable").emitDestroy();
hook.disable();
console.log("re-enabled init count:", initCount);
