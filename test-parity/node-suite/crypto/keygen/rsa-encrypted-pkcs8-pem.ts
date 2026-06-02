import * as crypto from "node:crypto";

const passphrase = "secret passphrase";
const data = Buffer.from("encrypted pkcs8 rsa parity");

const pair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 1024,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: {
    type: "pkcs8",
    format: "pem",
    cipher: "aes-256-cbc",
    passphrase,
  },
});

console.log("sync public marker:", String(pair.publicKey).includes("BEGIN PUBLIC KEY"));
console.log(
  "sync private encrypted marker:",
  String(pair.privateKey).includes("BEGIN ENCRYPTED PRIVATE KEY"),
);

const privateKey = crypto.createPrivateKey({
  key: pair.privateKey,
  format: "pem",
  passphrase: Buffer.from(passphrase),
});
const signature = crypto.sign("sha256", data, privateKey);
console.log("sync encrypted verify:", crypto.verify("sha256", data, pair.publicKey, signature));

const asyncPair = await new Promise<{ publicKey: string; privateKey: string }>((resolve, reject) => {
  crypto.generateKeyPair(
    "rsa",
    {
      modulusLength: 1024,
      publicKeyEncoding: { type: "spki", format: "pem" },
      privateKeyEncoding: {
        type: "pkcs8",
        format: "pem",
        cipher: "aes-256-cbc",
        passphrase,
      },
    },
    (err, publicKey, privateKey) => {
      if (err) reject(err);
      else resolve({ publicKey, privateKey });
    },
  );
});

console.log("async public marker:", String(asyncPair.publicKey).includes("BEGIN PUBLIC KEY"));
console.log(
  "async private encrypted marker:",
  String(asyncPair.privateKey).includes("BEGIN ENCRYPTED PRIVATE KEY"),
);

const asyncPrivateKey = crypto.createPrivateKey({
  key: asyncPair.privateKey,
  format: "pem",
  passphrase,
});
const asyncSignature = crypto.sign("sha256", data, asyncPrivateKey);
console.log("async encrypted verify:", crypto.verify("sha256", data, asyncPair.publicKey, asyncSignature));
