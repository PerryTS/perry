import * as crypto from "node:crypto";
import { randomBytes } from "node:crypto";
import { Buffer } from "node:buffer";

await new Promise<void>((resolve) => {
  crypto.randomBytes(12, (err, buf) => {
    console.log("randomBytes async err:", err === null);
    console.log("randomBytes async is buffer:", Buffer.isBuffer(buf));
    console.log("randomBytes async length:", buf.length);
    console.log("randomBytes async nonzero length:", buf.toString("hex").length);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  randomBytes(8, (err, buf) => {
    console.log("randomBytes named async err:", err === null);
    console.log("randomBytes named async is buffer:", Buffer.isBuffer(buf));
    console.log("randomBytes named async length:", buf.length);
    resolve();
  });
});
