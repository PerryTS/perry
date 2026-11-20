import * as crypto from "node:crypto";

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const data = Buffer.from("rsa-pss parity data");
const options = {
  key: privateKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
};
const verifyOptions = {
  key: publicKey,
  padding: crypto.constants.RSA_PKCS1_PSS_PADDING,
  saltLength: 32,
};

const sig = crypto.sign("sha256", data, options);
console.log("pss sig length:", sig.length);
console.log("pss verify ok:", crypto.verify("sha256", data, verifyOptions, sig));
console.log("pss verify bad data:", crypto.verify("sha256", Buffer.from("bad"), verifyOptions, sig));
console.log("pss pkcs1 verify fails:", crypto.verify("sha256", data, publicKey, sig));
