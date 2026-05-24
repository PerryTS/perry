import { Readable } from "node:stream";
// Readable.from(iter, options) accepts a 2nd argument with stream options
// such as objectMode and highWaterMark.
const r = Readable.from(["a", "b"], { objectMode: false, highWaterMark: 1 });
console.log("readableObjectMode:", r.readableObjectMode);
console.log("readableHighWaterMark:", r.readableHighWaterMark);
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("data:", out.join(",")));
