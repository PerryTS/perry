import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const pair = crypto.generateKeyPairSync("ed25519");
const data = Buffer.from("ed25519 generated key signing data");
const signature = crypto.sign(undefined, data, pair.privateKey);
const ok = crypto.verify(undefined, data, pair.publicKey, signature);
const bad = crypto.verify(undefined, Buffer.from("tampered"), pair.publicKey, signature);
const derivedPublic = crypto.createPublicKey(pair.privateKey);
const derivedOk = crypto.verify(undefined, data, derivedPublic, signature);

console.log("ed25519 signature len:", signature.length);
console.log("ed25519 verify ok:", ok);
console.log("ed25519 verify bad:", bad);
console.log("ed25519 verify derived:", derivedOk);
