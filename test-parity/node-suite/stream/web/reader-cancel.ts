import { ReadableStream } from "node:stream/web";
// reader.cancel(reason) cancels the stream and resolves with undefined.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
});
const r = rs.getReader();
const result = await r.cancel("stop");
console.log("cancel returns:", result);
const next = await r.read();
console.log("done after cancel:", next.done);
