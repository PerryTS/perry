import { EventEmitterAsyncResource } from "node:events";

class AsyncEmitter extends EventEmitterAsyncResource {
    constructor() { super({ name: "named export" }); }
}
const asyncEmitter = new AsyncEmitter();
asyncEmitter.on("value", value => console.log("async subclass", value));
asyncEmitter.emit("value", 9);
console.log(asyncEmitter.asyncId > 0, asyncEmitter.asyncResource.eventEmitter === asyncEmitter);
asyncEmitter.emitDestroy();
