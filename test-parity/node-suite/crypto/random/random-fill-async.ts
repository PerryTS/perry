import * as crypto from "node:crypto";
import { randomFill } from "node:crypto";
import { Buffer } from "node:buffer";

await new Promise<void>((resolve) => {
  const buf = Buffer.alloc(10);
  crypto.randomFill(buf, 2, 4, (err, filled) => {
    console.log("randomFill buffer err:", err === null);
    console.log("randomFill buffer same:", filled === buf);
    console.log("randomFill buffer length:", buf.length);
    console.log("randomFill buffer prefix zero:", buf[0] === 0 && buf[1] === 0);
    console.log("randomFill buffer suffix zero:", buf[6] === 0 && buf[9] === 0);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  const arr = new Uint8Array(4);
  randomFill(arr, (err, filled) => {
    console.log("randomFill typed err:", err === null);
    console.log("randomFill typed same:", filled === arr);
    console.log("randomFill typed completed:", true);
    resolve();
  });
});
