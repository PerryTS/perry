import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const pair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { format: "jwk" },
  privateKeyEncoding: { format: "jwk" },
});

const publicJwk = pair.publicKey as JsonWebKey;
const privateJwk = pair.privateKey as JsonWebKey;
const data = Buffer.from("rsa generateKeyPairSync jwk parity");
const signature = crypto.sign("RSA-SHA256", data, { key: privateJwk, format: "jwk" });
const encrypted = crypto.publicEncrypt({ key: publicJwk, format: "jwk" }, data);

console.log("rsa keygen jwk kty:", publicJwk.kty, privateJwk.kty);
console.log("rsa keygen jwk alg:", publicJwk.alg, privateJwk.alg);
console.log("rsa keygen jwk crt:", !!privateJwk.p, !!privateJwk.q, !!privateJwk.dp, !!privateJwk.dq, !!privateJwk.qi);
console.log("rsa keygen jwk verify:", crypto.verify("RSA-SHA256", data, { key: publicJwk, format: "jwk" }, signature));
console.log("rsa keygen jwk decrypt:", crypto.privateDecrypt({ key: privateJwk, format: "jwk" }, encrypted).toString());
