import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

for (const alg of ["sha1", "sha224", "sha256", "sha384", "sha512"]) {
  const out = Buffer.from(crypto.hkdfSync(alg, "ikm", "salt", "info", 12));
  console.log(alg + ":", out.length, out.toString("hex"));
}
