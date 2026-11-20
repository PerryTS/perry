import {
  AsyncResource,
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";

let initId = -1;
let initResource: object | undefined;
let initTrigger = -1;
const root = executionAsyncId();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "SubclassResource") {
      initId = asyncId;
      initResource = resource;
      initTrigger = triggerAsyncId;
    }
  },
}).enable();

class ScopedResource extends AsyncResource {
  readonly label: string;
  constructor(label: string) {
    super("SubclassResource");
    this.label = label;
  }
  invoke<T>(fn: (label: string) => T): T {
    return this.runInAsyncScope(fn, this, this.label);
  }
}

const resource = new ScopedResource("value");
const result = resource.invoke(function (this: ScopedResource, label) {
  return [
    this === resource,
    label === "value",
    executionAsyncResource() === resource,
    executionAsyncId() === resource.asyncId(),
  ].join(",");
});
console.log(
  "subclass shape:",
  resource instanceof ScopedResource,
  resource instanceof AsyncResource,
);
console.log(
  "subclass init:",
  initId === resource.asyncId(),
  initResource === resource,
  initTrigger === root,
);
console.log("subclass scope:", result);
resource.emitDestroy();
hook.disable();
