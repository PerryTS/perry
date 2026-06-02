import { ReadableStream, CountQueuingStrategy } from "node:stream/web";
// new ReadableStream(source, strategy) — both args.
const rs = new ReadableStream(
  { start(c) { c.enqueue("x"); c.close(); } },
  new CountQueuingStrategy({ highWaterMark: 5 }),
);
const reader = rs.getReader();
const a = await reader.read();
const b = await reader.read();
console.log("a:", a.value);
console.log("b done:", b.done);
