import * as crypto from "node:crypto";

const dh = crypto.createDiffieHellman(512);
console.log("dh generateKeys name:", dh.generateKeys.name);
console.log("dh computeSecret name:", dh.computeSecret.name);
console.log("dh getPrime name:", dh.getPrime.name);
console.log("dh getGenerator name:", dh.getGenerator.name);
console.log("dh getPublicKey name:", dh.getPublicKey.name);
console.log("dh getPrivateKey name:", dh.getPrivateKey.name);
console.log("dh setPublicKey name:", dh.setPublicKey.name);
console.log("dh setPrivateKey name:", dh.setPrivateKey.name);
console.log("dh generateKeys typeof:", typeof dh.generateKeys);
console.log("dh computeSecret typeof:", typeof dh.computeSecret);
