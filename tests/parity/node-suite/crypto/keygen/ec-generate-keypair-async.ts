import * as crypto from "node:crypto";

await new Promise<void>((resolve) => {
  crypto.generateKeyPair("ec", {
    namedCurve: "prime256v1",
    publicKeyEncoding: { type: "spki", format: "pem" },
    privateKeyEncoding: { type: "pkcs8", format: "pem" },
  }, (err, publicKey, privateKey) => {
    console.log("ec async err:", err === null);
    console.log("ec async public pem:", publicKey.includes("BEGIN PUBLIC KEY"));
    console.log("ec async private pem:", privateKey.includes("BEGIN PRIVATE KEY"));
    const msg = Buffer.from("async ec");
    const sig = crypto.sign("sha256", msg, privateKey);
    console.log("ec async verify:", crypto.verify("sha256", msg, publicKey, sig));
    resolve();
  });
});
