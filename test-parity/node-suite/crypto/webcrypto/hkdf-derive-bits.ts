import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const ec = new TextEncoder();
  const cases = [
    ["SHA-256", 512, "14d93b0ccd99d4f2cbd9fbfe9c830b5b8a43e3e45e32941ef21bdeb0fa87b6b6bfa5c54466aa5bf76cdc2685fba4408ea5b94c049fe035649b46f92fdc519374"],
    ["SHA-384", 128, "e36cf2cf943d8f3a88adb80f478745c3"],
  ] as const;
  for (const [hash, length, expected] of cases) {
    const key = await crypto.subtle.importKey("raw", ec.encode("hello"), { name: "HKDF", hash }, false, ["deriveBits"]);
    const bits = await crypto.subtle.deriveBits(
      { name: "HKDF", hash, salt: ec.encode("my friend"), info: ec.encode("there") },
      key,
      length,
    );
    console.log(`hkdf ${hash} bits hex:`, Buffer.from(bits).toString("hex"));
    console.log(`hkdf ${hash} matches:`, Buffer.from(bits).toString("hex") === expected);
  }
}
await main();
