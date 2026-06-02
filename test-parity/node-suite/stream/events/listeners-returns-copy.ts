import { Readable } from "node:stream";
// listeners(event) returns a COPY of the listener array — mutating it
// must not affect the emitter's internal list.
const r = new Readable({ read() {} });
const fn1 = () => {};
const fn2 = () => {};
r.on("data", fn1);
r.on("data", fn2);
const arr = r.listeners("data");
arr.length = 0; // mutate the returned array
console.log("internal count:", r.listenerCount("data"));
console.log("unchanged:", r.listenerCount("data") === 2);
