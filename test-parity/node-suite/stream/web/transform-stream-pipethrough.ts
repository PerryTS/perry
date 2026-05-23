import { ReadableStream, TransformStream } from "node:stream/web";
// rs.pipeThrough(ts) returns the transform's readable side, so you can chain.
const rs = new ReadableStream({
  start(c) { c.enqueue("hi"); c.close(); },
});
const ts = new TransformStream();
const piped = rs.pipeThrough(ts);
console.log("returns readable:", piped instanceof ReadableStream);
