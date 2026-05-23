import { Readable } from "node:stream";
// In objectMode, read() returns one object per call (not a Buffer concat).
const r = new Readable({ objectMode: true, read() {} });
r.push({ a: 1 });
r.push({ b: 2 });
r.push(null);
const first = r.read();
const second = r.read();
console.log("first.a:", first && first.a);
console.log("second.b:", second && second.b);
