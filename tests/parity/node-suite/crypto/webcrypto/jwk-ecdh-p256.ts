import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const alice = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveBits"],
  );
  const bob = await crypto.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveBits"],
  );
  const alicePrivateJwk = await crypto.subtle.exportKey("jwk", alice.privateKey) as JsonWebKey;
  const bobPublicJwk = await crypto.subtle.exportKey("jwk", bob.publicKey) as JsonWebKey;
  console.log("ecdh jwk public:", bobPublicJwk.kty, bobPublicJwk.crv, !!bobPublicJwk.x, !!bobPublicJwk.y, !!(bobPublicJwk as any).d);
  console.log("ecdh jwk private:", alicePrivateJwk.kty, alicePrivateJwk.crv, !!alicePrivateJwk.x, !!alicePrivateJwk.y, !!alicePrivateJwk.d);
  const importedAlicePrivate = await crypto.subtle.importKey("jwk", alicePrivateJwk, { name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]);
  const importedBobPublic = await crypto.subtle.importKey("jwk", bobPublicJwk, { name: "ECDH", namedCurve: "P-256" }, true, []);
  const bits = await crypto.subtle.deriveBits({ name: "ECDH", public: importedBobPublic }, importedAlicePrivate, 256);
  console.log("ecdh jwk bits len:", Buffer.from(bits).length);
}
await main();
