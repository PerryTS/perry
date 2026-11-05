import { Readable } from "node:stream";
// Stream constructor exposes a fresh object — each new() makes distinct instances.
const r1 = new Readable({ read() {} });
const r2 = new Readable({ read() {} });
console.log("distinct instances:", r1 !== r2);
console.log("both readable:", r1.readable && r2.readable);
console.log("not same proto:", Object.getPrototypeOf(r1) === Object.getPrototypeOf(r2));
