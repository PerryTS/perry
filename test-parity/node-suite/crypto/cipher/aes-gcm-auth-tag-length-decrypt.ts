import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.alloc(32, 11);
const iv = Buffer.alloc(12, 12);
const aad = Buffer.from("gcm aad");
const plain = Buffer.from("gcm truncated tag decrypt plaintext");

for (const authTagLength of [16, 15, 14, 13, 12, 8, 4]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength });
  cipher.setAAD(aad);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  const tag = cipher.getAuthTag();

  const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv, { authTagLength });
  decipher.setAAD(aad);
  decipher.setAuthTag(tag);
  const decrypted = Buffer.concat([decipher.update(ciphertext), decipher.final()]);

  console.log("gcm decrypt tag length:", authTagLength, tag.length);
  console.log("gcm decrypt roundtrip:", authTagLength, decrypted.equals(plain));
}

for (const [algorithm, variantKey, authTagLength] of [
  ["aes-128-gcm", Buffer.alloc(16, 13), 8],
  ["aes-192-gcm", Buffer.alloc(24, 14), 4],
] as const) {
  const cipher = crypto.createCipheriv(algorithm, variantKey, iv, { authTagLength });
  cipher.setAAD(aad);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  const tag = cipher.getAuthTag();

  const decipher = crypto.createDecipheriv(algorithm, variantKey, iv, { authTagLength });
  decipher.setAAD(aad);
  decipher.setAuthTag(tag);
  const decrypted = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
  console.log(`gcm ${algorithm} short decrypt roundtrip:`, authTagLength, decrypted.equals(plain));
}

// Non-96-bit IV vectors generated with Node.js crypto using the
// plaintext/AAD constants above; regenerate if those fixtures change.
for (const vector of [
  {
    algorithm: "aes-256-gcm",
    key: "2929292929292929292929292929292929292929292929292929292929292929",
    iv: "6e6f6e2d39362d6269742d6976",
    authTagLength: 8,
    ciphertext: "b9a67988c5a704f5cd89018551c24a71f2bde99a4b713b58902d82709f58cb43e02059",
    tag: "591fbc55bcdded22",
  },
  {
    algorithm: "aes-256-gcm",
    key: "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a",
    iv: "73686f7274697631",
    authTagLength: 4,
    ciphertext: "23d22e2f52ed4215b743e35a7638cd7a5e97f7e3269c2b56c864b52dbb860befcc247b",
    tag: "c8787702",
  },
]) {
  const decipher = crypto.createDecipheriv(
    vector.algorithm,
    Buffer.from(vector.key, "hex"),
    Buffer.from(vector.iv, "hex"),
    { authTagLength: vector.authTagLength },
  );
  decipher.setAAD(aad);
  decipher.setAuthTag(Buffer.from(vector.tag, "hex"));
  const decrypted = Buffer.concat([
    decipher.update(Buffer.from(vector.ciphertext, "hex")),
    decipher.final(),
  ]);
  console.log("gcm non-96-bit IV short decrypt:", vector.authTagLength, decrypted.equals(plain));
}

for (const authTagLength of [8, 4]) {
  const cipher = crypto.createCipheriv("aes-256-gcm", key, iv, { authTagLength });
  cipher.setAAD(aad);
  const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
  const tag = cipher.getAuthTag();
  const badTag = Buffer.concat([Buffer.from([tag[0] ^ 0xff]), tag.subarray(1)]);

  const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv, { authTagLength });
  decipher.setAAD(aad);
  decipher.setAuthTag(badTag);
  let accepted = false;
  try {
    accepted = Buffer.concat([decipher.update(ciphertext), decipher.final()]).equals(plain);
  } catch {
    accepted = false;
  }
  console.log("gcm decrypt tampered short tag:", authTagLength, accepted ? "unexpected-success" : "failed");
}
