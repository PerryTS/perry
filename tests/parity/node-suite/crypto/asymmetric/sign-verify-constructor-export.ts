import * as crypto from "node:crypto";
import { Sign, Verify } from "node:crypto";

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const data = "constructor sign verify";
const signer = Sign("RSA-SHA256");
console.log("sign constructor update returns this:", signer.update(data) === signer);
const sig = signer.sign(privateKey);
console.log("sign constructor sig len:", sig.length > 128);
const verifier = Verify("RSA-SHA256");
console.log("verify constructor update returns this:", verifier.update(data) === verifier);
console.log("verify constructor ok:", verifier.verify(publicKey, sig));
