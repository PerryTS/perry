import { Readable } from "node:stream";
// The asyncIterator's return() method exists and is callable to bail early.
const r = Readable.from([1, 2, 3]);
const it = (r as any)[Symbol.asyncIterator]();
console.log("has return:", typeof it.return === "function");
const result = await it.return();
console.log("return result done:", result.done);
console.log("destroyed:", r.destroyed);
