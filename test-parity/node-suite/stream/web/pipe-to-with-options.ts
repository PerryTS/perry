import { ReadableStream, WritableStream } from "node:stream/web";
// rs.pipeTo(ws, { preventClose }) keeps the destination open after the
// source ends.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
let closed = false;
const ws = new WritableStream({
  write() {},
  close() { closed = true; },
});
await rs.pipeTo(ws, { preventClose: true });
console.log("sink closed (should be false):", closed);
