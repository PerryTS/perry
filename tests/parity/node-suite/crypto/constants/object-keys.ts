import * as crypto from "node:crypto";
import c from "node:crypto";

// crypto.constants enumerates its OpenSSL constants in Node's order, not an internal key.
const keys = Object.keys(crypto.constants);
console.log("count:", keys.length);
console.log("keys:", keys.join(","));
console.log("default import agrees:", Object.keys(c.constants).join(",") === keys.join(","));
console.log("RSA_SSLV23_PADDING:", crypto.constants.RSA_SSLV23_PADDING);
console.log("defaultCipherList:", typeof crypto.constants.defaultCipherList);
console.log("cipher lists agree:", crypto.constants.defaultCipherList === crypto.constants.defaultCoreCipherList);

const copy = { ...crypto.constants };
console.log("spread:", Object.keys(copy).length, copy.SSL_OP_ALL, copy.RSA_PKCS1_PADDING);
let forIn = 0;
for (const _ in crypto.constants) forIn++;
console.log("for-in:", forIn);
