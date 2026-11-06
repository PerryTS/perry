import { EventEmitterAsyncResource } from "node:events";

const emitter = new EventEmitterAsyncResource({
  name: "ParityEventEmitterBackReference",
});
const resource = emitter.asyncResource;
console.log(
  "event resource back reference:",
  typeof resource,
  resource.eventEmitter === emitter,
);
emitter.emitDestroy();
