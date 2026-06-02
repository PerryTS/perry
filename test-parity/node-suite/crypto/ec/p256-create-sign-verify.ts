import * as crypto from "node:crypto";

const pair = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const signer = crypto.createSign("sha256");
signer.update("streamed p256 data");
signer.update(Buffer.from(" with buffer"));
const signature = signer.sign(pair.privateKey);

const verifier = crypto.createVerify("sha256");
verifier.update("streamed p256 data");
verifier.update(Buffer.from(" with buffer"));

console.log("ec streamed signature nonempty:", signature.length > 0);
console.log("ec streamed verify:", verifier.verify(pair.publicKey, signature));
