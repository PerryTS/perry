import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const b = Buffer.alloc(6);
const ret = crypto.randomFillSync(b, 2, 3);
console.log("same object:", ret === b);
console.log("len:", b.length);
console.log("prefix zero:", b[0] === 0 && b[1] === 0);
console.log("suffix zero:", b[5] === 0);
