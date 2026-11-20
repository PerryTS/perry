import { WritableStream } from "node:stream/web";
// writer.releaseLock() unlocks the stream so a new writer can be acquired.
const ws = new WritableStream({ write() {} });
const w1 = ws.getWriter();
console.log("locked-1:", ws.locked);
w1.releaseLock();
console.log("locked-2:", ws.locked);
const w2 = ws.getWriter();
console.log("can re-acquire:", typeof w2.write === "function");
