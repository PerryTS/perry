import { Readable } from "node:stream";
// listenerCount(event) returns 0 when no listener registered.
const r = new Readable({ read() {} });
console.log("no listeners:", r.listenerCount("data"));
console.log("is 0:", r.listenerCount("data") === 0);
