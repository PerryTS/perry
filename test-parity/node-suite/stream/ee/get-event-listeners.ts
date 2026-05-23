import { PassThrough } from "node:stream";
import { getEventListeners } from "node:events";
// events.getEventListeners(emitter, event) (Node 19+) returns the listener
// array, including for streams (which extend EventEmitter).
const p = new PassThrough();
const fn = () => {};
p.on("data", fn);
const listeners = getEventListeners(p, "data");
console.log("is array:", Array.isArray(listeners));
console.log("length:", listeners.length);
