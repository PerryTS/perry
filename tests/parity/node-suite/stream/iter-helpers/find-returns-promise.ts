import { Readable } from "node:stream";
// r.find(fn) returns a Promise — Promise.resolve(undefined) if no match.
const r = Readable.from([1, 2, 3, 4]);
const p = (r as any).find((x: number) => x > 2);
console.log("is Promise:", typeof p.then === "function");
const value = await p;
console.log("value:", value);
