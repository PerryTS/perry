import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );

  const publicJwk = await crypto.subtle.exportKey("jwk", pair.publicKey) as JsonWebKey;
  const privateJwk = await crypto.subtle.exportKey("jwk", pair.privateKey) as JsonWebKey;

  const privateKey = crypto.createPrivateKey({ key: privateJwk, format: "jwk" });
  const publicKey = crypto.createPublicKey({ key: publicJwk, format: "jwk" });
  const derivedPublicKey = crypto.createPublicKey({ key: privateJwk, format: "jwk" });

  const data = Buffer.from("ec jwk create keys parity");
  const signature = crypto.sign("sha256", data, privateKey);
  const directSignature = crypto.sign("sha256", data, { key: privateJwk, format: "jwk" });

  console.log("ec jwk public kty/crv:", publicJwk.kty, publicJwk.crv);
  console.log("ec jwk private has d:", !!privateJwk.d);
  console.log("ec jwk sig nonempty:", signature.length > 0);
  console.log("ec jwk verify public:", crypto.verify("sha256", data, publicKey, signature));
  console.log("ec jwk verify derived:", crypto.verify("sha256", data, derivedPublicKey, signature));
  console.log("ec jwk verify direct:", crypto.verify("sha256", data, { key: publicJwk, format: "jwk" }, directSignature));
  console.log("ec jwk verify bad:", crypto.verify("sha256", Buffer.from("tampered"), publicKey, signature));
}

await main();
