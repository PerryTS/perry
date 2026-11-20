import * as crypto from "node:crypto";

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const data1 = Buffer.from("rsa-pss streaming ");
const data2 = Buffer.from("parity data");

const signer = crypto.createSign("RSA-SHA384");
signer.update(data1);
signer.update(data2);
const sig = signer.sign({
  key: privateKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 48,
});

const verifier = crypto.createVerify("RSA-SHA384");
verifier.update(data1);
verifier.update(data2);
const ok = verifier.verify({
  key: publicKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 48,
}, sig);

const badVerifier = crypto.createVerify("RSA-SHA384");
badVerifier.update(data1);
badVerifier.update(Buffer.from("wrong"));
const bad = badVerifier.verify({
  key: publicKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 48,
}, sig);

console.log("stream pss sig length:", sig.length);
console.log("stream pss verify ok:", ok);
console.log("stream pss verify bad:", bad);
