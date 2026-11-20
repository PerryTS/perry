import { AsyncLocalStorage, createHook } from "node:async_hooks";
import { EventEmitterAsyncResource } from "node:events";

const storage = new AsyncLocalStorage<string>();
let target = -1;
let observedResource: object | undefined;
const lifecycle: string[] = [];
let emitter!: EventEmitterAsyncResource;
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type === "ParityEventEmitter") {
      target = asyncId;
      observedResource = resource;
      lifecycle.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === target) lifecycle.push("before");
  },
  after(asyncId) {
    if (asyncId === target) lifecycle.push("after");
  },
  destroy(asyncId) {
    if (asyncId === target) lifecycle.push("destroy");
  },
}).enable();

emitter = storage.run(
  "constructed",
  () => new EventEmitterAsyncResource({ name: "ParityEventEmitter" }),
);
const stores: string[] = [];
emitter.on("event", (value) => {
  stores.push(`${storage.getStore()}:${value}`);
});
storage.run("emitting", () => emitter.emit("event", "payload"));
emitter.emitDestroy();
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("event async resource id positive:", emitter.asyncId > 0);
console.log(
  "event async resource trigger nonnegative:",
  emitter.triggerAsyncId >= 0,
);
console.log("event async resource listener stores:", stores.join(","));
console.log("event async resource lifecycle:", lifecycle.join(">"));
console.log(
  "event async resource supplied:",
  observedResource === emitter.asyncResource,
);
console.log("event async resource outside:", String(storage.getStore()));
