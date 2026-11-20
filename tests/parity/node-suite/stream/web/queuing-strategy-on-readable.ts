import { ReadableStream, CountQueuingStrategy } from "node:stream/web";
// A CountQueuingStrategy passed to a ReadableStream sets its highWaterMark.
const rs = new ReadableStream(
  { start(c) { c.enqueue("x"); c.close(); } },
  new CountQueuingStrategy({ highWaterMark: 3 }),
);
const r = rs.getReader();
const first = await r.read();
console.log("first:", first.value);
const eof = await r.read();
console.log("done:", eof.done);
