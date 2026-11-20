import { ReadableStream } from "node:stream/web";
// ReadableStream.cancel(reason) propagates the reason to the underlying
// source's cancel() hook.
let seen: any = null;
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
  cancel(reason) { seen = reason; },
});
const r = rs.getReader();
await r.cancel("user-stop");
console.log("source saw:", seen);
console.log("rs.locked after release:", rs.locked);
