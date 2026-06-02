import { Readable } from "node:stream";
// Readable instances are async-iterable: r[Symbol.asyncIterator]() returns
// an async iterator.
const r = Readable.from(["x"]);
const it = (r as any)[Symbol.asyncIterator]();
console.log("has next:", typeof it.next === "function");
const v = await it.next();
console.log("value:", v.value);
