import { ReadableStream } from "node:stream/web";
// rs.cancel() returns Promise<undefined>.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); } });
const p = rs.cancel("stop");
console.log("is Promise:", typeof (p as any).then === "function");
const v = await p;
console.log("resolves to:", v);
console.log("is undefined:", v === undefined);
