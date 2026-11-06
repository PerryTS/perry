import * as crypto from "node:crypto";

for (const name of ["aes-128-cbc", "aes-192-cbc", "aes-256-cbc", "aes-128-gcm", "aes-192-gcm", "aes-256-gcm"]) {
  const info = crypto.getCipherInfo(name)!;
  console.log("cipher info:", info.name, info.keyLength, info.ivLength, info.blockSize, info.mode);
  console.log("cipher info by nid:", crypto.getCipherInfo(info.nid)?.name);
}

console.log("cipher missing:", crypto.getCipherInfo("cipher that does not exist"));
console.log("cipher key mismatch:", crypto.getCipherInfo("aes-128-cbc", { keyLength: 12 }));
console.log("cipher key match:", !!crypto.getCipherInfo("aes-128-cbc", { keyLength: 16 }));
const ivOverride = crypto.getCipherInfo("aes-256-gcm", { ivLength: 16 });
console.log("cipher iv override:", ivOverride?.name, ivOverride?.ivLength);
console.log("cipher iv match:", !!crypto.getCipherInfo("aes-256-gcm", { ivLength: 12 }));
