import { WritableStream } from "node:stream/web";
// WritableStream supports start/write/close/abort hooks; each fires at its
// expected lifecycle point.
const order: string[] = [];
const ws = new WritableStream({
  start() { order.push("start"); },
  write() { order.push("write"); },
  close() { order.push("close"); },
  abort() { order.push("abort"); },
});
const w = ws.getWriter();
await w.write("x");
await w.close();
console.log("order:", order.join(","));
