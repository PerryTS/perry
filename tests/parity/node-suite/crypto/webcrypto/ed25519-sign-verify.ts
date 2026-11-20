import * as crypto from "node:crypto";

async function main() {
  const pair = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const data = new TextEncoder().encode("webcrypto ed25519 parity");
  const signature = await crypto.subtle.sign({ name: "Ed25519" }, pair.privateKey, data);
  const ok = await crypto.subtle.verify({ name: "Ed25519" }, pair.publicKey, signature, data);
  const bad = await crypto.subtle.verify({ name: "Ed25519" }, pair.publicKey, signature, new TextEncoder().encode("tampered"));
  const rawPublic = await crypto.subtle.exportKey("raw", pair.publicKey);
  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "Ed25519" }, true, ["verify"]);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "Ed25519" }, true, ["sign"]);
  const importedSignature = await crypto.subtle.sign("Ed25519", importedPrivate, data);

  console.log("ed25519 sig len:", (signature as ArrayBuffer).byteLength);
  console.log("ed25519 verify ok:", ok);
  console.log("ed25519 verify bad:", bad);
  console.log("ed25519 raw public len:", (rawPublic as ArrayBuffer).byteLength);
  console.log("ed25519 jwk kty/crv:", publicJwk.kty, publicJwk.crv, privateJwk.crv);
  console.log("ed25519 jwk has d:", !!privateJwk.d);
  console.log("ed25519 imported verify:", await crypto.subtle.verify("Ed25519", importedPublic, importedSignature, data));
}

await main();
