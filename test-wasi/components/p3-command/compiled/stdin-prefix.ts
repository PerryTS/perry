// Return before stdin EOF, with the completion future still unread. Task-local
// fs teardown must release only its duplicate, not libc's instance-owned stdin.
import { readSync, writeSync, closeSync } from "node:fs";
const byte = new Uint8Array(1);
console.log("prefix", readSync(0, byte, 0, 1, null), byte[0]);
writeSync(1, Buffer.from("registry stdout\n"));
closeSync(0);
closeSync(1);
closeSync(2);
console.log("standard streams retained");
