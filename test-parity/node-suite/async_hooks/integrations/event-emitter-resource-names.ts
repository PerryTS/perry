import { createHook, executionAsyncId } from "node:async_hooks";
import { EventEmitterAsyncResource } from "node:events";

type InitInfo = { triggerAsyncId: number; resource: object };
const observed = new Map<string, InitInfo>();
const tracked = new Set(["NamedEmitter", "ExplicitString", "ExplicitOptions"]);
const hook = createHook({
  init(_asyncId, type, triggerAsyncId, resource) {
    if (tracked.has(type)) observed.set(type, { triggerAsyncId, resource });
  },
}).enable();

class NamedEmitter extends EventEmitterAsyncResource {}
const parentId = executionAsyncId();
const defaultEmitter = new NamedEmitter();
const stringEmitter = new NamedEmitter("ExplicitString");
const optionsEmitter = new NamedEmitter({
  name: "ExplicitOptions",
  triggerAsyncId: 73,
});

const executionMatches: boolean[] = [];
for (const emitter of [defaultEmitter, stringEmitter, optionsEmitter]) {
  emitter.on("event", () => {
    executionMatches.push(executionAsyncId() === emitter.asyncId);
  });
  emitter.emit("event");
}

console.log(
  "event resource names observed:",
  [...observed.keys()].sort().join(","),
);
console.log(
  "event resource default triggers:",
  observed.get("NamedEmitter")?.triggerAsyncId === parentId,
  observed.get("ExplicitString")?.triggerAsyncId === parentId,
  observed.get("ExplicitOptions")?.triggerAsyncId === 73,
);
console.log(
  "event resource objects supplied:",
  observed.get("NamedEmitter")?.resource === defaultEmitter.asyncResource,
  observed.get("ExplicitString")?.resource === stringEmitter.asyncResource,
  observed.get("ExplicitOptions")?.resource === optionsEmitter.asyncResource,
);
console.log("event resource execution ids:", executionMatches.join(","));

for (const emitter of [defaultEmitter, stringEmitter, optionsEmitter]) {
  emitter.emitDestroy();
}
hook.disable();
