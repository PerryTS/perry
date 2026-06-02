import * as crypto from "node:crypto";

for (const name of ["aes-128-ecb", "aes-192-ecb", "aes-256-ecb", "id-aes128-wrap", "aes192-wrap", "id-aes256-wrap"]) {
  const info = crypto.getCipherInfo(name)!;
  console.log("cipher extra info:", name, info.name, info.keyLength, info.blockSize, info.mode, info.ivLength);
  console.log("cipher extra by nid:", crypto.getCipherInfo(info.nid)?.name);
}
console.log("ciphers has ecb:", crypto.getCiphers().includes("aes-128-ecb"));
console.log("ciphers has wrap:", crypto.getCiphers().includes("id-aes128-wrap"));
