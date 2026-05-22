import * as crypto from "node:crypto";

await new Promise<void>((resolve) => {
  crypto.generateKey("aes", { length: 192 }, (err, key) => {
    console.log("generateKey aes err:", err === null);
    console.log("generateKey aes type:", key.type);
    console.log("generateKey aes size:", key.symmetricKeySize);
    console.log("generateKey aes export length:", key.export().length);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  crypto.generateKey("hmac", { length: 123 }, (err, key) => {
    console.log("generateKey hmac err:", err === null);
    console.log("generateKey hmac type:", key.type);
    console.log("generateKey hmac size:", key.symmetricKeySize);
    console.log("generateKey hmac export length:", key.export().length);
    resolve();
  });
});
