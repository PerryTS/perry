import { ReadableStream } from "node:stream/web";
// new ReadableStream({ type: 'bytes' }) marks the stream as a byte stream
// (supports BYOB readers).
const rs = new ReadableStream({
  type: "bytes",
  start(c: any) { c.enqueue(new Uint8Array([1, 2])); c.close(); },
});
const reader = rs.getReader();
const v = await reader.read();
console.log("done after one chunk:", v.done === false);
const v2 = await reader.read();
console.log("done after end:", v2.done);
