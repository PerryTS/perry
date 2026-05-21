import { Buffer } from "node:buffer";

const buf = Buffer.from([0x61, 0x62, 0x63, 0x64, 0x65, 0x66]);
const s = buf.slice(1, 4);

// Mutating the slice should be visible through the original buffer.
s[0] = 0x5a;
console.log("buf[1]:", buf[1].toString(16));
console.log("s[0]:", s[0].toString(16));

// Mutating the original buffer should be visible through the slice.
buf[2] = 0x5b;
console.log("buf[2]:", buf[2].toString(16));
console.log("s[1]:", s[1].toString(16));
