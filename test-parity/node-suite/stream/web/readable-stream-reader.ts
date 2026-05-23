import { ReadableStream } from "node:stream/web";
// new ReadableStream({ start }) + getReader().read() drains the stream
// chunk-by-chunk until { done: true }.
const rs = new ReadableStream({
  start(c) {
    c.enqueue("a");
    c.enqueue("b");
    c.close();
  },
});
const reader = rs.getReader();
const a = await reader.read();
console.log("first:", a.value);
const b = await reader.read();
console.log("second:", b.value);
const eof = await reader.read();
console.log("done:", eof.done);
