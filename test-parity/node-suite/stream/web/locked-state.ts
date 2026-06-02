import { ReadableStream } from "node:stream/web";
// A ReadableStream becomes locked once getReader() is acquired.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
console.log("before:", rs.locked);
const r = rs.getReader();
console.log("after:", rs.locked);
r.releaseLock();
console.log("after release:", rs.locked);
