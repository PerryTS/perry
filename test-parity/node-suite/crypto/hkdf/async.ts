import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

await new Promise<void>((resolve) => {
  crypto.hkdf("sha256", "ikm", "salt", "info", 16, (err, key) => {
    console.log("hkdf async sha256 err:", err === null);
    console.log("hkdf async sha256 length:", Buffer.from(key).length);
    console.log("hkdf async sha256 hex:", Buffer.from(key).toString("hex"));
    resolve();
  });
});
