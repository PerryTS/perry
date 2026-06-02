import { Readable } from "node:stream";
// In objectMode a pushed Buffer is delivered as the same Buffer — not
// concatenated or decoded.
const r = new Readable({ objectMode: true, read() {} });
const original = Buffer.from("xyz");
r.on("data", (chunk: any) => console.log("same:", chunk === original));
r.push(original);
r.push(null);
