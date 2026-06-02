import * as crypto from "node:crypto";

await new Promise<void>((resolve) => {
  crypto.generateKeyPair("x25519", {}, (err, publicKey, privateKey) => {
    console.log("x25519 async err:", err === null);
    const other = crypto.generateKeyPairSync("x25519");
    const secret = crypto.diffieHellman({ privateKey, publicKey: other.publicKey });
    console.log("x25519 async secret length:", Buffer.from(secret).length);
    resolve();
  });
});
