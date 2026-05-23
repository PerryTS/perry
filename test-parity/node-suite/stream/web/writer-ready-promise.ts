import { WritableStream } from "node:stream/web";
// writer.ready is a Promise that resolves when the writer can accept the
// next write (backpressure release).
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
const p = w.ready;
console.log("ready is promise:", typeof (p as any).then === "function");
await p;
console.log("ready resolved");
