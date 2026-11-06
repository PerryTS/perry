import { Readable } from "node:stream";
// After [Symbol.asyncIterator]() fully consumes the stream, a second call
// returns an iterator that's immediately done.
const r = Readable.from([1, 2, 3]);
const first: any[] = [];
for await (const v of r) first.push(v);
const it2 = (r as any)[Symbol.asyncIterator]();
const next = await it2.next();
console.log("first:", first.join(","));
console.log("second iter done:", next.done);
