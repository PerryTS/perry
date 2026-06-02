import { WritableStream } from "node:stream/web";
// A WritableStream can only have one writer locked at a time; getWriter()
// throws if already locked.
const ws = new WritableStream({ write() {} });
ws.getWriter(); // lock
let threw = false;
try {
  ws.getWriter();
} catch {
  threw = true;
}
console.log("second getWriter threw:", threw);
