import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough(transform, options) — all four options accepted (preventClose,
// preventAbort, preventCancel, signal).
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ts = new TransformStream();
const result = rs.pipeThrough(ts, {
  preventClose: false,
  preventAbort: false,
  preventCancel: false,
});
console.log("returned RS:", result instanceof ReadableStream);
// Drain to consume
const reader = result.getReader();
const { value } = await reader.read();
console.log("first value:", value);
