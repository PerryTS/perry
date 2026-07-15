import { AsyncResource, createHook } from "node:async_hooks";

let resourceId = -1;
let destroyCount = 0;

const hook = createHook({
  init(asyncId, type) {
    if (type === "ParityDestroy") resourceId = asyncId;
  },
  destroy(asyncId) {
    if (asyncId === resourceId) destroyCount++;
  },
}).enable();

const resource = new AsyncResource("ParityDestroy");
console.log("init matched resource:", resourceId === resource.asyncId());
console.log("destroy return identity:", resource.emitDestroy() === resource);
resource.emitDestroy();

setImmediate(() => {
  console.log("destroy callback count:", destroyCount);
  hook.disable();
});
