import * as crypto from "node:crypto";

const original = crypto.createDiffieHellman(512);
const peer = crypto.createDiffieHellman(original.getPrime(), "buffer");
const originalPublic = original.generateKeys();
const originalPrivate = original.getPrivateKey();
const peerPublicHex = peer.generateKeys("hex");

const clone = crypto.createDiffieHellman(original.getPrime(), "buffer");
clone.setPrivateKey(originalPrivate);
clone.setPublicKey(originalPublic);

const originalSecret = original.computeSecret(peerPublicHex, "hex", "base64");
const cloneSecret = clone.computeSecret(peerPublicHex, "hex", "base64");

console.log("dh public restored:", Buffer.from(clone.getPublicKey()).toString("hex") === Buffer.from(originalPublic).toString("hex"));
console.log("dh private restored:", Buffer.from(clone.getPrivateKey()).toString("hex") === Buffer.from(originalPrivate).toString("hex"));
console.log("dh restored secret:", originalSecret === cloneSecret);
