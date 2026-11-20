import * as crypto from "node:crypto";

await new Promise<void>((resolve) => {
  crypto.generateKeyPair("rsa", {
    modulusLength: 2048,
    publicKeyEncoding: { type: "spki", format: "pem" },
    privateKeyEncoding: { type: "pkcs8", format: "pem" },
  }, (err, publicKey, privateKey) => {
    console.log("rsa async err:", err === null);
    console.log("rsa async public pem:", publicKey.includes("BEGIN PUBLIC KEY"));
    console.log("rsa async private pem:", privateKey.includes("BEGIN PRIVATE KEY"));
    const msg = Buffer.from("async rsa");
    const sig = crypto.sign("sha256", msg, privateKey);
    console.log("rsa async verify:", crypto.verify("sha256", msg, publicKey, sig));
    resolve();
  });
});
