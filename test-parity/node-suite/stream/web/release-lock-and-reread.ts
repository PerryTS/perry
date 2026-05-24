import { ReadableStream } from "node:stream/web";
// After releaseLock(), a new getReader() should succeed (stream unlocked).
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const r1 = rs.getReader();
const first = await r1.read();
r1.releaseLock();
console.log("locked after release:", rs.locked);
const r2 = rs.getReader();
const second = await r2.read();
console.log("first value:", first.value);
console.log("second value:", second.value);
