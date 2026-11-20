import { WritableStream } from "node:stream/web";
// WritableStream.abort() returns Promise<void>.
const ws = new WritableStream({ write() {}, abort() {} });
const p = ws.abort("stop");
console.log("is Promise:", typeof (p as any).then === "function");
const v = await p;
console.log("resolves to:", v);
console.log("is undefined:", v === undefined);
