import { Readable } from "node:stream";
// emitter.listeners(event) returns the array of registered listeners.
const r = new Readable({ read() {} });
const fn1 = () => {};
const fn2 = () => {};
r.on("custom", fn1);
r.on("custom", fn2);
const arr = r.listeners("custom");
console.log("count:", arr.length);
console.log("is array:", Array.isArray(arr));
console.log("contains fn1:", arr.includes(fn1));
console.log("contains fn2:", arr.includes(fn2));
