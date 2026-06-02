import { WritableStream } from "node:stream/web";
// Sink can define start, write, close, abort — all 4 are honored.
const order: string[] = [];
const ws = new WritableStream({
  start() { order.push("start"); },
  write(c) { order.push("write:" + String(c)); },
  close() { order.push("close"); },
  abort() { order.push("abort"); },
});
const w = ws.getWriter();
await w.write("x");
await w.close();
console.log("order:", order.join(","));
