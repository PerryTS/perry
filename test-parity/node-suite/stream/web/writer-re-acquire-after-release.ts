import { WritableStream } from "node:stream/web";
// After releaseLock(), getWriter() can be called again.
const ws = new WritableStream({ write() {} });
const w1 = ws.getWriter();
w1.releaseLock();
console.log("locked after release:", ws.locked);
const w2 = ws.getWriter();
console.log("re-acquired:", w2 !== w1);
console.log("locked again:", ws.locked);
await w2.close();
