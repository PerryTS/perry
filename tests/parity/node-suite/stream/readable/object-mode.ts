import { Readable } from "node:stream";
// objectMode: true lets the stream push arbitrary JS values (not just
// Buffers/strings) and emit them one per 'data' event.
const r = new Readable({ objectMode: true, read() {} });
const out: any[] = [];
r.on("data", (v) => out.push(v));
r.on("end", () => console.log("count + first:", out.length, JSON.stringify(out[0])));
r.push({ a: 1 });
r.push({ b: 2 });
r.push(null);
