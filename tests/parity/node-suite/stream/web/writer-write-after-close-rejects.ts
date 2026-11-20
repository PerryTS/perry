import { WritableStream } from "node:stream/web";
// Calling write() after close() should reject — the writer is closed.
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
await w.close();
let rejected = false;
try {
  await w.write("late");
} catch {
  rejected = true;
}
console.log("rejected:", rejected);
