import * as crypto from "node:crypto";

const original = crypto.createECDH("prime256v1");
const restored = crypto.createECDH("prime256v1");
const peer = crypto.createECDH("prime256v1");

const originalPublic = original.generateKeys("hex");
const privateKey = original.getPrivateKey("hex");
restored.setPrivateKey(privateKey, "hex");
const restoredPublic = restored.getPublicKey("hex");
const peerPublic = peer.generateKeys("hex");

const originalSecret = original.computeSecret(peerPublic, "hex", "hex");
const restoredSecret = restored.computeSecret(peerPublic, "hex", "hex");

console.log("public restored:", originalPublic === restoredPublic);
console.log("secret restored:", originalSecret === restoredSecret);
