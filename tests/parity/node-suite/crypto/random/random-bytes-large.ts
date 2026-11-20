import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const oneMiB = 1024 * 1024;

const exact = crypto.randomBytes(oneMiB);
const over = crypto.randomBytes(oneMiB + 1);

console.log("sync exact length:", exact.length);
console.log("sync over length:", over.length);
console.log("sync over buffer:", Buffer.isBuffer(over));

await new Promise<void>((resolve) => {
  crypto.randomBytes(oneMiB + 1, (err, buf) => {
    console.log("async err null:", err === null);
    console.log("async length:", buf.length);
    console.log("async buffer:", Buffer.isBuffer(buf));
    resolve();
  });
});
