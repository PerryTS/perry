import * as crypto from "node:crypto";

const ecdh = crypto.createECDH("prime256v1");
console.log("ecdh generateKeys name:", ecdh.generateKeys.name);
console.log("ecdh computeSecret name:", ecdh.computeSecret.name);
console.log("ecdh getPublicKey name:", ecdh.getPublicKey.name);
console.log("ecdh getPrivateKey name:", ecdh.getPrivateKey.name);
console.log("ecdh setPublicKey name:", ecdh.setPublicKey.name);
console.log("ecdh setPrivateKey name:", ecdh.setPrivateKey.name);
console.log("ecdh generateKeys typeof:", typeof ecdh.generateKeys);
console.log("ecdh computeSecret typeof:", typeof ecdh.computeSecret);
