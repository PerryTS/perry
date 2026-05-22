import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";

const pair = generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});

const privateKey = createPrivateKey(pair.privateKey);
const publicKey = createPublicKey(privateKey);
const data = Buffer.from("ec key object surrogate");
const signature = sign("sha256", data, privateKey);

console.log("ec keyobject verify:", verify("sha256", data, publicKey, signature));
console.log("ec keyobject signature nonempty:", signature.length > 0);
