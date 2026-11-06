import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const pair = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const data = Buffer.from("p256 dsa encoding parity");
const derSignature = crypto.sign("sha256", data, { key: pair.privateKey, dsaEncoding: "der" });
const p1363Signature = crypto.sign("sha256", data, { key: pair.privateKey, dsaEncoding: "ieee-p1363" });

const signer = crypto.createSign("sha256");
signer.update(data);
const streamingP1363 = signer.sign({ key: pair.privateKey, dsaEncoding: "ieee-p1363" });

const verifier = crypto.createVerify("sha256");
verifier.update(data);

console.log("ec der sig variable:", derSignature.length > 0 && derSignature.length <= 72 && derSignature.length !== 64);
console.log("ec p1363 sig len:", p1363Signature.length);
console.log("ec p1363 verify:", crypto.verify("sha256", data, { key: pair.publicKey, dsaEncoding: "ieee-p1363" }, p1363Signature));
console.log("ec p1363 verify bad:", crypto.verify("sha256", Buffer.from("tampered"), { key: pair.publicKey, dsaEncoding: "ieee-p1363" }, p1363Signature));
console.log("ec p1363 streaming len:", streamingP1363.length);
console.log("ec p1363 streaming verify:", verifier.verify({ key: pair.publicKey, dsaEncoding: "ieee-p1363" }, streamingP1363));
