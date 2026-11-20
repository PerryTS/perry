import { ReadableStream } from "node:stream/web";
// reader.closed resolves when the stream closes (or rejects on error).
const rs = new ReadableStream({
  start(c) { c.close(); },
});
const r = rs.getReader();
await r.closed;
console.log("closed resolved");
