import { Readable } from "node:stream";
// readableLength counts BYTES (UTF-8) for string pushes, not characters.
const r = new Readable({ read() {} });
r.push("é"); // 2 bytes in UTF-8
console.log("length:", r.readableLength);
console.log("is 2:", r.readableLength === 2);
