import * as crypto from "node:crypto";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "RSA-PSS", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );

  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
  console.log("pss public kty/alg:", publicJwk.kty, publicJwk.alg);
  console.log("pss private has crt:", !!privateJwk.p, !!privateJwk.q, !!privateJwk.dp, !!privateJwk.dq, !!privateJwk.qi);

  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "RSA-PSS", hash: "SHA-256" }, true, ["verify"]);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "RSA-PSS", hash: "SHA-256" }, true, ["sign"]);
  const data = new TextEncoder().encode("rsa pss jwk data");
  const sig = await crypto.subtle.sign({ name: "RSA-PSS", saltLength: 32 }, importedPrivate, data);
  console.log("pss jwk verify:", await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, importedPublic, sig, data));
}
await main();
