import { Readable } from "node:stream";
// Readable.isDisturbed(stream) reports whether a Readable has been read /
// errored / destroyed.
const r = new Readable({ read() {} });
console.log("before:", Readable.isDisturbed(r));
r.push("x");
r.read();
console.log("after read:", Readable.isDisturbed(r));
