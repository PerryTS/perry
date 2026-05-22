import { createDiffieHellman, getDiffieHellman, createDiffieHellmanGroup } from "node:crypto";

const first = createDiffieHellman(512);
const second = createDiffieHellman(first.getPrime(), "buffer");
const firstPub = first.generateKeys();
const secondPub = second.generateKeys();
console.log("named dh secret equal:", first.computeSecret(secondPub).toString("hex") === second.computeSecret(firstPub).toString("hex"));

const groupA = createDiffieHellmanGroup("modp5");
const groupB = getDiffieHellman("modp5");
groupA.generateKeys();
groupB.generateKeys();
console.log("named group secret equal:", groupA.computeSecret(groupB.getPublicKey()).toString("hex") === groupB.computeSecret(groupA.getPublicKey()).toString("hex"));
