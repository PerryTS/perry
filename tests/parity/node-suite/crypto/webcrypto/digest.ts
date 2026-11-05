import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const data = new TextEncoder().encode("abc");
  for (const alg of ["SHA-1", "SHA-256", "SHA-384", "SHA-512"]) {
    const digest = await crypto.subtle.digest(alg, data);
    console.log(alg + ":", Buffer.from(digest).toString("hex"));
  }
}
await main();
