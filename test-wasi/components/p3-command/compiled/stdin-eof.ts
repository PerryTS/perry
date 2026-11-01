import { readSync, closeSync } from "node:fs";
const byte = new Uint8Array(1);
let sum = 0;
while (readSync(0, byte, 0, 1, null) !== 0) sum += byte[0];
closeSync(0);
console.log("eof", sum);
