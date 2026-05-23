import { WritableStream } from "node:stream/web";
// WritableStream.abort(reason) returns a Promise that resolves once the
// stream has been aborted; subsequent writes reject.
const ws = new WritableStream({ write() {} });
await ws.abort("stop");
const writer = ws.getWriter();
let rejected = false;
try { await writer.write("post-abort"); } catch { rejected = true; }
console.log("post-abort write rejected:", rejected);
