import * as crypto from "node:crypto";

async function main() {
  const alice = await crypto.subtle.generateKey({ name: "X25519" }, true, ["deriveBits"]);
  const bob = await crypto.subtle.generateKey({ name: "X25519" }, true, ["deriveBits"]);
  const secret1 = await crypto.subtle.deriveBits({ name: "X25519", public: alice.publicKey }, bob.privateKey, 128);
  const secret2 = await crypto.subtle.deriveBits({ name: "X25519", public: bob.publicKey }, alice.privateKey, 128);
  const rawPublic = await crypto.subtle.exportKey("raw", alice.publicKey);
  const publicJwk = await crypto.subtle.exportKey("jwk", alice.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", alice.privateKey) as JsonWebKey;
  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "X25519" }, true, []);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "X25519" }, true, ["deriveBits"]);
  const importedSecret = await crypto.subtle.deriveBits({ name: "X25519", public: importedPublic }, importedPrivate, 128);

  console.log("x25519 secret len:", (secret1 as ArrayBuffer).byteLength);
  console.log("x25519 secret equal:", Buffer.from(secret1).equals(Buffer.from(secret2)));
  console.log("x25519 raw public len:", (rawPublic as ArrayBuffer).byteLength);
  console.log("x25519 jwk kty/crv:", publicJwk.kty, publicJwk.crv, privateJwk.crv);
  console.log("x25519 jwk has d:", !!privateJwk.d);
  console.log("x25519 imported self secret len:", (importedSecret as ArrayBuffer).byteLength);
}

await main();
