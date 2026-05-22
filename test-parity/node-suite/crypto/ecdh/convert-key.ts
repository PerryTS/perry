import * as crypto from "node:crypto";

const ecdh = crypto.createECDH("prime256v1");
ecdh.generateKeys();
const uncompressed = ecdh.getPublicKey("hex", "uncompressed");
const compressed = ecdh.getPublicKey("hex", "compressed");

const convertedCompressed = crypto.ECDH.convertKey(
  uncompressed,
  "prime256v1",
  "hex",
  "hex",
  "compressed",
);
const convertedUncompressed = crypto.ECDH.convertKey(
  compressed,
  "prime256v1",
  "hex",
  "hex",
  "uncompressed",
);
const convertedBuffer = crypto.ECDH.convertKey(
  compressed,
  "prime256v1",
  "hex",
  "buffer",
  "compressed",
);
const convertedBase64 = crypto.ECDH.convertKey(
  compressed,
  "prime256v1",
  "hex",
  "base64",
  "compressed",
);

console.log("compressed matches:", convertedCompressed === compressed);
console.log("uncompressed matches:", convertedUncompressed === uncompressed);
console.log("buffer length:", convertedBuffer.length);
console.log("base64 roundtrip:", Buffer.from(convertedBase64, "base64").toString("hex") === compressed);
