import { Readable } from "node:stream";
// A fresh Readable: listenerCount('data') is 0; listenerCount('error') is 0.
const r = new Readable({ read() {} });
console.log("data:", r.listenerCount("data"));
console.log("error:", r.listenerCount("error"));
console.log("end:", r.listenerCount("end"));
console.log("readable:", r.listenerCount("readable"));
console.log("all zero:",
  r.listenerCount("data") === 0 &&
  r.listenerCount("error") === 0 &&
  r.listenerCount("end") === 0);
