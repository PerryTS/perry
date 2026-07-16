import { AsyncResource, createHook, executionAsyncId } from "node:async_hooks";

let resourceId = -1;
let beforeCount = 0;
let afterCount = 0;
let destroyCount = 0;
const hook = createHook({
  before(asyncId) {
    if (asyncId === resourceId) beforeCount++;
  },
  after(asyncId) {
    if (asyncId === resourceId) afterCount++;
  },
  destroy(asyncId) {
    if (asyncId === resourceId) destroyCount++;
  },
}).enable();

const resource = new AsyncResource("PostDestroyScope");
resourceId = resource.asyncId();
resource.emitDestroy();
const scopeId = resource.runInAsyncScope(() => executionAsyncId());

setImmediate(() => {
  console.log(
    "post-destroy scope lifecycle:",
    scopeId === resourceId,
    beforeCount,
    afterCount,
    destroyCount,
  );
  hook.disable();
});
