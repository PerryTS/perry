import { AsyncResource, createHook } from "node:async_hooks";

const accesses: string[] = [];
const options = Object.create(null, {
  triggerAsyncId: {
    get() {
      accesses.push("triggerAsyncId");
      return 73;
    },
  },
  requireManualDestroy: {
    get() {
      accesses.push("requireManualDestroy");
      return true;
    },
  },
});
let initTrigger = -1;
let initResource: object | undefined;
const hook = createHook({
  init(_asyncId, type, triggerAsyncId, resource) {
    if (type === "AccessorOptionsResource") {
      initTrigger = triggerAsyncId;
      initResource = resource;
    }
  },
}).enable();

const resource = new AsyncResource("AccessorOptionsResource", options);
const triggerMethod = (resource as any).triggerAsyncId;
const trigger =
  typeof triggerMethod === "function"
    ? triggerMethod.call(resource)
    : "missing";
console.log("resource option accessor order:", accesses.join(","));
console.log(
  "resource option values:",
  trigger,
  initTrigger,
  initResource === resource,
);
resource.emitDestroy();
hook.disable();
