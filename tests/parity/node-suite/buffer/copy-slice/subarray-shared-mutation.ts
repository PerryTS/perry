import { Buffer } from "node:buffer";

const buf = Buffer.from([0x61, 0x62, 0x63, 0x64, 0x65, 0x66]);
const v = buf.subarray(1, 4);

// Mutating the original buffer must be visible through the view.
buf[1] = 0x62;
console.log("buf[1]:", buf[1].toString(16));
console.log("v[0]:", v[0].toString(16));

// Mutating the view must be visible through the original buffer.
v[1] = 0x59;
console.log("buf[2]:", buf[2].toString(16));
console.log("v[1]:", v[1].toString(16));
