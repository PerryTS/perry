import * as crypto from "node:crypto";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
  console.log("ecdsa jwk public:", publicJwk.kty, publicJwk.crv, !!publicJwk.x, !!publicJwk.y, !!(publicJwk as any).d);
  console.log("ecdsa jwk private:", privateJwk.kty, privateJwk.crv, !!privateJwk.x, !!privateJwk.y, !!privateJwk.d);
  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "ECDSA", namedCurve: "P-256" }, true, ["verify"]);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "ECDSA", namedCurve: "P-256" }, true, ["sign"]);
  const data = new TextEncoder().encode("ecdsa jwk data");
  const sig = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, importedPrivate, data);
  console.log("ecdsa jwk verify:", await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, importedPublic, sig, data));
}
await main();
