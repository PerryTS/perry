import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["encrypt", "decrypt"],
  );

  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;
  console.log("oaep public kty/alg:", publicJwk.kty, publicJwk.alg);
  console.log("oaep public fields:", !!publicJwk.n, !!publicJwk.e, !!(publicJwk as any).d);
  console.log("oaep private fields:", !!privateJwk.d, !!privateJwk.p, !!privateJwk.q, !!privateJwk.dp, !!privateJwk.dq, !!privateJwk.qi);

  const importedPublic = await crypto.subtle.importKey("jwk", publicJwk, { name: "RSA-OAEP", hash: "SHA-256" }, true, ["encrypt"]);
  const importedPrivate = await crypto.subtle.importKey("jwk", privateJwk, { name: "RSA-OAEP", hash: "SHA-256" }, true, ["decrypt"]);
  const ct = await crypto.subtle.encrypt("RSA-OAEP", importedPublic, new TextEncoder().encode("rsa oaep jwk"));
  const pt = await crypto.subtle.decrypt("RSA-OAEP", importedPrivate, ct);
  console.log("oaep jwk pt:", Buffer.from(pt).toString());
}
await main();
