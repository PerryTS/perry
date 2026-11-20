import { AsyncResource, createHook } from "node:async_hooks";

let observedResource: unknown;
let observedType = "";

const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "ParityResourceArgument") {
      observedType = type;
      observedResource = resource;
    }
  },
}).enable();

const resource = new AsyncResource("ParityResourceArgument");
console.log("resource argument type:", observedType);
console.log("resource argument identity:", observedResource === resource);
console.log("resource argument object:", typeof observedResource);

hook.disable();
resource.emitDestroy();
