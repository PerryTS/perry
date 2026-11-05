import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo() returns a Promise<void> that resolves when pipe completes
// (or rejects on error).
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws);
console.log("is Promise:", typeof (p as any).then === "function");
const v = await p;
console.log("resolves to:", v);
console.log("is undefined:", v === undefined);
