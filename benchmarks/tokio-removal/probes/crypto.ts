import * as crypto from "node:crypto";
import { promisify } from "node:util";
async function main() {
  const pb = await promisify(crypto.pbkdf2)("pw", "salt", 1000, 16, "sha256");
  console.log("pbkdf2", pb.toString("hex"));
  const sc = await promisify(crypto.scrypt)("pw", "salt", 16);
  console.log("scrypt", (sc as Buffer).toString("hex"));
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("abc"));
  console.log("digest", Buffer.from(d).toString("hex"));
  const rb = await promisify(crypto.randomBytes)(8);
  console.log("randomBytes", rb.length);
  console.log("hash", crypto.createHash("sha1").update("x").digest("hex"));
}
main();
