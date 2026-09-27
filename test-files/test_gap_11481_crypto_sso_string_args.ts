// #11481: crypto string arguments built at runtime and short enough to be
// stored inline (SSO) were masked to 48 bits and dereferenced as a heap
// `StringHeader*`, segfaulting in `bytes_from_ptr`. Every value below is
// computed at runtime so none of them can be folded into a heap literal.
import * as crypto from "node:crypto";

const n = 3;
const short = "x" + (n & 7); // "x3"
const hexEnc = "he" + String.fromCharCode(120); // "hex"
const alg = "sha" + (n - 2); // "sha1"
const key = "k" + (n & 1); // "k1"
const hexData = "78" + (30 + n); // "7833" === hex of "x3"

// hash: update data, update input encoding, digest encoding, algorithm.
console.log(crypto.createHash("sha1").update(short).digest("hex"));
console.log(crypto.createHash("sha1").update(short).digest(hexEnc));
console.log(crypto.createHash(alg).update(short).digest("hex"));
console.log(crypto.createHash("sha1").update(hexData, hexEnc).digest("hex"));

// hmac: key, data, digest encoding.
console.log(crypto.createHmac("sha256", key).update(short).digest("hex"));
console.log(crypto.createHmac(alg, key).update(short).digest(hexEnc));

// kdfs: password/salt.
console.log(crypto.pbkdf2Sync(key, short, 1, 16, alg).toString("hex"));
console.log(crypto.scryptSync(key, short, 16).toString("hex"));

// cipher.update with no input encoding.
const cipher = crypto.createCipheriv(
  "aes-128-cbc",
  "0123456789abcdef",
  "fedcba9876543210",
);
const ct = Buffer.concat([cipher.update(short), cipher.final()]);
console.log(ct.toString("hex"));
const decipher = crypto.createDecipheriv(
  "aes-128-cbc",
  "0123456789abcdef",
  "fedcba9876543210",
);
console.log(
  Buffer.concat([decipher.update(ct), decipher.final()]).toString("utf8"),
);
