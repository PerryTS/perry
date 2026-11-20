import crypto from "node:crypto";

// RFC 8439 section 2.8.2 AEAD test vector.
const key = Buffer.from(
  "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f",
  "hex",
);
const iv = Buffer.from("070000004041424344454647", "hex");
const aad = Buffer.from("50515253c0c1c2c3c4c5c6c7", "hex");
const plaintext = Buffer.from(
  "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.",
);

const cipher = crypto.createCipheriv("chacha20-poly1305", key, iv);
cipher.setAAD(aad);
const first = cipher.update(plaintext.subarray(0, 40));
const second = cipher.update(plaintext.subarray(40));
const last = cipher.final();
const ciphertext = Buffer.concat([first, second, last]);
const tag = cipher.getAuthTag();
console.log("chunks:", first.length, second.length, last.length);
console.log("ciphertext:", ciphertext.toString("hex"));
console.log("tag:", tag.toString("hex"));

const decipher = crypto.createDecipheriv("chacha20-poly1305", key, iv);
decipher.setAAD(aad);
decipher.setAuthTag(tag);
const recovered = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
console.log("roundtrip:", recovered.equals(plaintext));

function decryptOutcome(label: string, build: (d: crypto.DecipherGCM) => void) {
  const d = crypto.createDecipheriv("chacha20-poly1305", key, iv) as crypto.DecipherGCM;
  try {
    build(d);
    Buffer.concat([d.update(ciphertext), d.final()]);
    console.log(label, "accepted");
  } catch (err: any) {
    console.log(label, "rejected:", err.message);
  }
}

decryptOutcome("wrong aad", (d) => {
  d.setAAD(Buffer.from("other"));
  d.setAuthTag(tag);
});
decryptOutcome("flipped tag", (d) => {
  d.setAAD(aad);
  const bad = Buffer.from(tag);
  bad[0] ^= 1;
  d.setAuthTag(bad);
});

// A short tag is the prefix of the full one.
const short = crypto.createCipheriv("chacha20-poly1305", key, iv, { authTagLength: 8 });
short.setAAD(aad);
const shortCiphertext = Buffer.concat([short.update(plaintext), short.final()]);
const shortTag = short.getAuthTag();
console.log("short tag:", shortTag.length, shortTag.equals(tag.subarray(0, 8)));
console.log("short ciphertext same:", shortCiphertext.equals(ciphertext));
const shortDecipher = crypto.createDecipheriv("chacha20-poly1305", key, iv, {
  authTagLength: 8,
});
shortDecipher.setAAD(aad);
shortDecipher.setAuthTag(shortTag);
console.log(
  "short roundtrip:",
  Buffer.concat([shortDecipher.update(shortCiphertext), shortDecipher.final()]).equals(plaintext),
);

// String input and output encodings.
const text = crypto.createCipheriv("chacha20-poly1305", key, iv);
const hex = text.update("hello", "utf8", "hex") + text.final("hex");
console.log("hex:", hex, text.getAuthTag().toString("hex"));

const info = crypto.getCipherInfo("chacha20-poly1305")!;
console.log("info:", info.name, info.nid, info.keyLength, info.ivLength, info.mode, info.blockSize);
console.log("by nid:", crypto.getCipherInfo(1018)?.name);
console.log("listed:", crypto.getCiphers().includes("chacha20-poly1305"));
