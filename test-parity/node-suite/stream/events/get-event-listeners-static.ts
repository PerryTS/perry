import { Readable } from "node:stream";
import { getEventListeners } from "node:events";
// events.getEventListeners(emitter, event) returns the array of listeners
// for the named event on the emitter.
const r = new Readable({ read() {} });
const fn1 = () => {};
const fn2 = () => {};
r.on("custom", fn1);
r.on("custom", fn2);
const listeners = getEventListeners(r, "custom");
console.log("count:", listeners.length);
console.log("is array:", Array.isArray(listeners));
console.log("contains fn1:", listeners.includes(fn1));
console.log("contains fn2:", listeners.includes(fn2));
