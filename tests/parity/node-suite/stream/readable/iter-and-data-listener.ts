import { Readable } from "node:stream";
// Calling for-await on a stream that ALREADY has a 'data' listener
// works: iteration consumes; the 'data' listener also sees the same data.
const r = Readable.from(["a", "b"]);
const dataEvents: string[] = [];
r.on("data", (c) => dataEvents.push(String(c)));
const iter: string[] = [];
for await (const v of r) iter.push(String(v));
console.log("iter:", iter.join(","));
console.log("data events:", dataEvents.length);
