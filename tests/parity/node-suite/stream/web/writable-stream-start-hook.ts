import { WritableStream } from "node:stream/web";
// The underlying sink's start() runs before any write — and a Promise it
// returns must settle before write() is invoked.
const events: string[] = [];
const ws = new WritableStream({
  async start() {
    events.push("start");
  },
  write(chunk) {
    events.push("write:" + String(chunk));
  },
});
const writer = ws.getWriter();
await writer.write("x");
await writer.close();
console.log("order:", events.join(","));
console.log("start before write:", events.indexOf("start") < events.indexOf("write:x"));
