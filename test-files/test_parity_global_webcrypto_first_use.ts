// `globalThis.crypto` is created by the runtime, not at a `node:crypto`
// namespace site, so its native-module dispatch bucket was never installed
// when the global's methods ran first: `crypto.subtle.digest(...)` threw
// "crypto.subtle.digest is not supported". The node:crypto namespace is only
// used after the global calls.
import * as nodeCrypto from "node:crypto";
const enc = new TextEncoder().encode("abc");
const hex = (b: ArrayBuffer) =>
  Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, "0")).join("");
console.log("digest", hex(await globalThis.crypto.subtle.digest("SHA-384", enc)).slice(0, 32));
const key = await globalThis.crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]);
const sig = await globalThis.crypto.subtle.sign("HMAC", key, enc);
console.log("hmac", sig.byteLength, await globalThis.crypto.subtle.verify("HMAC", key, sig, enc));
console.log("uuid", /^[0-9a-f-]{36}$/.test(globalThis.crypto.randomUUID()));
console.log("node", nodeCrypto.createHash("sha256").update("abc").digest("hex").slice(0, 16));
