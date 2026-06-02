import { WritableStream } from "node:stream/web";
// writer.closed is a Promise that resolves when the writer / stream closes.
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
const p = w.closed;
console.log("is promise:", typeof (p as any).then === "function");
await w.close();
await p;
console.log("closed resolved");
