import crypto from "node:crypto";
// randomFillSync fills the given buffer in place and returns it (value is
// non-deterministic; length/identity are fixed).
const buf = Buffer.alloc(8);
const ret = crypto.randomFillSync(buf);
console.log("length:", buf.length);
console.log("returns same buffer:", ret === buf);
