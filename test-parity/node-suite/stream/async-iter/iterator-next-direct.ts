import { Readable } from "node:stream";
// r[Symbol.asyncIterator]() returns an async iterator whose next() yields
// {value, done} Promises.
const r = Readable.from(["a", "b"]);
const it = (r as any)[Symbol.asyncIterator]();
const a = await it.next();
const b = await it.next();
const c = await it.next();
console.log("a:", a.value, a.done);
console.log("b:", b.value, b.done);
console.log("c done:", c.done);
