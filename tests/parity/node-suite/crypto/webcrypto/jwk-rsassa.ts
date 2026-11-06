import * as crypto from "node:crypto";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );

  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
  console.log("rsassa public kty/alg:", publicJwk.kty, publicJwk.alg);
  console.log("rsassa private has crt:", !!privateJwk.p, !!privateJwk.q, !!privateJwk.dp, !!privateJwk.dq, !!privateJwk.qi);

  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["sign"]);
  const data = new TextEncoder().encode("rsassa jwk data");
  const sig = await crypto.subtle.sign({ name: "RSASSA-PKCS1-v1_5" }, importedPrivate, data);
  console.log("rsassa jwk verify:", await crypto.subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, importedPublic, sig, data));
}
await main();
