import { WritableStream } from "node:stream/web";
// writer.close() returns a Promise that resolves once the stream is fully closed.
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
const p = w.close();
console.log("is promise:", typeof (p as any).then === "function");
await p;
console.log("closed");
