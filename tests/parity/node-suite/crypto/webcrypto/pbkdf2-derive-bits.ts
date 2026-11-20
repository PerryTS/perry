import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const ec = new TextEncoder();
  const cases = [
    ["SHA-256", 10, 512, "f72d1cf4853fffbd16a42751765d11f8dc7939498ee7b7ce7678b4cb16fad88098110a83e71f4483ce73203f7a64719d293280f780f9fafdcf46925c5c0588b3"],
    ["SHA-384", 5, 128, "201509b012c9cd2fbe7ea938f0c509b3"],
  ] as const;
  for (const [hash, iterations, length, expected] of cases) {
    const key = await crypto.subtle.importKey("raw", ec.encode("hello"), { name: "PBKDF2", hash }, false, ["deriveBits"]);
    const bits = await crypto.subtle.deriveBits(
      { name: "PBKDF2", hash, salt: ec.encode("there"), iterations },
      key,
      length,
    );
    console.log(`pbkdf2 ${hash} bits hex:`, Buffer.from(bits).toString("hex"));
    console.log(`pbkdf2 ${hash} matches:`, Buffer.from(bits).toString("hex") === expected);
  }
}
await main();
