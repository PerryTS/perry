import { Readable } from "node:stream";
// Readable.from(iter, {objectMode: true}) — explicit objectMode.
const r = Readable.from(["a"], { objectMode: true });
console.log("objectMode:", r.readableObjectMode);
console.log("hwm:", r.readableHighWaterMark);
const out: any[] = [];
r.on("data", (c) => out.push(c));
r.on("end", () => console.log("count:", out.length));
