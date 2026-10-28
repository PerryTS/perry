import { EventEmitter, EventEmitterAsyncResource } from "node:events";
const EE = EventEmitter;
const Bound = EE.bind(null);
const emitter = new Bound();
emitter.on("value", value => console.log("bound", value));
emitter.emit("value", 4);
class Bus extends EE {}
const bus = new Bus();
bus.on("value", value => console.log("subclass", value));
bus.emit("value", 7);
console.log(bus instanceof EE, bus instanceof Bus, emitter instanceof EE);

class AsyncEmitter extends EventEmitterAsyncResource {
    constructor() { super({ name: "named export" }); }
}
const asyncEmitter = new AsyncEmitter();
asyncEmitter.on("value", value => console.log("async subclass", value));
asyncEmitter.emit("value", 9);
console.log(asyncEmitter.asyncId > 0, asyncEmitter.asyncResource.eventEmitter === asyncEmitter);
asyncEmitter.emitDestroy();
