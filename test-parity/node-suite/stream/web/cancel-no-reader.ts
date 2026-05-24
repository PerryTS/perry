import { ReadableStream } from "node:stream/web";
// rs.cancel() works directly without first calling getReader() — it
// cancels the stream and rejects future reads.
let cancelled = false;
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
  cancel() { cancelled = true; },
});
await rs.cancel("done");
console.log("cancelled hook fired:", cancelled);
console.log("locked after cancel:", rs.locked);
