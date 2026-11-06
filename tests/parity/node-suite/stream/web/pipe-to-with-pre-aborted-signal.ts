import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo({signal}) with a signal that was already aborted before the call
// rejects the returned Promise immediately.
const ctrl = new AbortController();
ctrl.abort();
const rs = new ReadableStream({ start(c) { c.enqueue("x"); } });
const ws = new WritableStream({ write() {} });
let rejected = false;
try {
  await rs.pipeTo(ws, { signal: ctrl.signal });
} catch {
  rejected = true;
}
console.log("rejected pre-aborted:", rejected);
