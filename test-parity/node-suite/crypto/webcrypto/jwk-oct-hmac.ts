import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const jwk = { kty: "oct", k: Buffer.from("secret-jwk-hmac").toString("base64url") };
  const key = await crypto.subtle.importKey("jwk", jwk, { name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]);
  const exported = await crypto.subtle.exportKey("jwk", key);
  console.log("jwk hmac kty:", exported.kty);
  console.log("jwk hmac roundtrip:", exported.k === jwk.k);
  const data = new TextEncoder().encode("jwk hmac data");
  const sig = await crypto.subtle.sign("HMAC", key, data);
  console.log("jwk hmac verify:", await crypto.subtle.verify("HMAC", key, sig, data));
}
await main();
