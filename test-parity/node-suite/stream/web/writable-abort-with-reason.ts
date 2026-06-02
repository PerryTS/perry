import { WritableStream } from "node:stream/web";
// WritableStream.abort(reason) propagates the reason to the underlying
// sink's abort() hook.
let seen: any = null;
const ws = new WritableStream({
  write() {},
  abort(reason) { seen = reason; },
});
await ws.abort("stop-now");
console.log("sink saw:", seen);
