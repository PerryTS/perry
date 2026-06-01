import {
  EventEmitter,
  EventEmitterAsyncResource,
} from "node:events";
import * as events from "node:events";
import { executionAsyncId, triggerAsyncId } from "node:async_hooks";

console.log(
  "export:",
  Object.prototype.hasOwnProperty.call(events, "EventEmitterAsyncResource"),
  "EventEmitterAsyncResource" in events,
  typeof EventEmitterAsyncResource,
  Object.keys(events).includes("EventEmitterAsyncResource"),
);
console.log(
  "static identity:",
  (EventEmitter as any).EventEmitterAsyncResource === EventEmitterAsyncResource,
  (events.default as any).EventEmitterAsyncResource === EventEmitterAsyncResource,
);

const resource = new EventEmitterAsyncResource({
  name: "perry-resource",
  triggerAsyncId: 7,
});
console.log(
  "instanceof:",
  resource instanceof EventEmitterAsyncResource,
  resource instanceof EventEmitter,
);
console.log(
  "ids:",
  typeof (resource as any).asyncId,
  Number.isInteger((resource as any).asyncId),
  typeof (resource as any).triggerAsyncId,
  (resource as any).triggerAsyncId,
);
console.log("asyncResource:", typeof (resource as any).asyncResource);

const seen: string[] = [];
resource.on("tick", function (this: any, value: string) {
  seen.push(
    [
      this === resource,
      value,
      executionAsyncId() === (resource as any).asyncId,
      triggerAsyncId() === (resource as any).triggerAsyncId,
    ].join(":"),
  );
});
console.log("emit return:", resource.emit("tick", "value"));
console.log("seen:", seen.join(","));
console.log(
  "eventNames/listenerCount:",
  resource.eventNames().join(","),
  resource.listenerCount("tick"),
);
console.log(
  "emitDestroy:",
  (resource as any).emitDestroy() === resource,
  typeof (resource as any).emitDestroy(),
);
console.log("plain emitDestroy:", typeof (new EventEmitter() as any).emitDestroy);
