import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

await new Promise<void>((resolve) => {
  crypto.pbkdf2("password", "salt", 1, 32, "sha256", (err, key) => {
    console.log("pbkdf2 async sha256 err:", err === null);
    console.log("pbkdf2 async sha256 hex:", Buffer.from(key).toString("hex"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  crypto.pbkdf2(Buffer.from("password"), Buffer.from("salt"), 2, 16, "sha512", (err, key) => {
    console.log("pbkdf2 async sha512 err:", err === null);
    console.log("pbkdf2 async sha512 length:", Buffer.from(key).length);
    console.log("pbkdf2 async sha512 hex:", Buffer.from(key).toString("hex"));
    resolve();
  });
});
