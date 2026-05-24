import { ReadableStream, TransformStream } from "node:stream/web";
// After pipeThrough(), the source is locked (transform took the reader).
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ts = new TransformStream();
rs.pipeThrough(ts);
console.log("rs locked after pipeThrough:", rs.locked);
