import { Readable } from "node:stream";

// Symbols with the same description are still distinct event names.
const r = new Readable({ read() {} });
const a = Symbol("shared");
const b = Symbol("shared");
let seenA = 0;
let seenB = 0;

r.on(a, () => seenA++);
r.on(b, () => seenB++);

r.emit(a);

console.log("a count:", seenA);
console.log("b count:", seenB);
console.log("listenerCount a:", r.listenerCount(a));
console.log("listenerCount b:", r.listenerCount(b));
console.log("eventNames length:", r.eventNames().length);
