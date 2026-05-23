import { ReadableStream } from "node:stream/web";
// cancel() releases the stream and rejects subsequent reads with done=true.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
});
await rs.cancel();
const reader = rs.getReader();
const r = await reader.read();
console.log("done after cancel:", r.done);
