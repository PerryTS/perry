import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

await new Promise<void>((resolve) => {
  crypto.scrypt("password", "salt", 16, (err, key) => {
    console.log("scrypt async err:", err === null);
    console.log("scrypt async length:", Buffer.from(key).length);
    console.log("scrypt async hex:", Buffer.from(key).toString("hex"));
    resolve();
  });
});
