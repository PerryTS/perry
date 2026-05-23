import { Readable } from "node:stream";
// After calling read() on buffered data, unshift() puts a chunk back so
// the next read() returns it first.
const r = new Readable({ read() {} });
r.push(Buffer.from("hello"));
r.push(null);
const a = r.read(2);
r.unshift(Buffer.from("X"));
const b = r.read();
console.log("a:", a && a.toString());
console.log("b:", b && b.toString());
