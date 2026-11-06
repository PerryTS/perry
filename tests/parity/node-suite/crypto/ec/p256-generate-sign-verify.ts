import * as crypto from "node:crypto";

const pair = crypto.generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const data = Buffer.from("p256 generated key signing data");
const signature = crypto.sign("sha256", data, pair.privateKey);
const ok = crypto.verify("sha256", data, pair.publicKey, signature);
const bad = crypto.verify("sha256", Buffer.from("tampered"), pair.publicKey, signature);

console.log("ec public string:", typeof pair.publicKey);
console.log("ec private string:", typeof pair.privateKey);
console.log("ec signature nonempty:", signature.length > 0);
console.log("ec verify ok:", ok);
console.log("ec verify bad:", bad);
