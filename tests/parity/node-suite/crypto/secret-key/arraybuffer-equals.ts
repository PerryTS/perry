import * as crypto from "node:crypto";
const first = crypto.createSecretKey(Buffer.alloc(0));
const second = crypto.createSecretKey(new ArrayBuffer(0));
const third = crypto.createSecretKey(Buffer.alloc(1));
console.log("empty buffer equals arraybuffer:", first.equals(second));
console.log("empty vs one:", first.equals(third));
