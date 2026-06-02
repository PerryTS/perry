import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const pair = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { format: "jwk" },
  privateKeyEncoding: { format: "jwk" },
});

const publicJwk = pair.publicKey as JsonWebKey;
const privateJwk = pair.privateKey as JsonWebKey;
const data = Buffer.from("ec generateKeyPairSync jwk parity");
const signature = crypto.sign("sha256", data, { key: privateJwk, format: "jwk" });

console.log("ec keygen jwk kty/crv:", publicJwk.kty, publicJwk.crv, privateJwk.crv);
console.log("ec keygen jwk private has d:", !!privateJwk.d);
console.log("ec keygen jwk verify:", crypto.verify("sha256", data, { key: publicJwk, format: "jwk" }, signature));
console.log("ec keygen jwk verify bad:", crypto.verify("sha256", Buffer.from("tampered"), { key: publicJwk, format: "jwk" }, signature));
