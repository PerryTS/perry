import { ReadableStream, WritableStream } from "node:stream/web";

let closed = false;
const rs = new ReadableStream({
  start(c) {
    c.enqueue("x");
    c.close();
  },
});
const ws = new WritableStream({
  write() {},
  async close() {
    await new Promise((resolve) => setTimeout(resolve, 10));
    closed = true;
  },
});

const p = rs.pipeTo(ws);
console.log("closed before await:", closed);
await p;
console.log("closed after await:", closed);
