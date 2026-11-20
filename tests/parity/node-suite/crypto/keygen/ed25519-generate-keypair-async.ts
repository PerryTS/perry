import * as crypto from "node:crypto";

await new Promise<void>((resolve) => {
  crypto.generateKeyPair("ed25519", {}, (err, publicKey, privateKey) => {
    console.log("ed25519 async err:", err === null);
    const msg = Buffer.from("async ed25519");
    const sig = crypto.sign(undefined, msg, privateKey);
    console.log("ed25519 async verify:", crypto.verify(undefined, msg, publicKey, sig));
    resolve();
  });
});
