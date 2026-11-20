import * as crypto from "node:crypto";

const key = "secret-key";
const input = "hello world";
const algorithms = ["sha1", "sha-256", "sHA-256", "sHa-256", "mD5", "sha256", "sha512", "md5"];

for (const alg of algorithms) {
  const direct = crypto.createHmac(alg, key).update(input).digest("hex");
  const split = crypto.createHmac(alg, key).update("hello").update(" ").update("world").digest("hex");
  console.log(`hmac ${alg} len:`, direct.length);
  console.log(`hmac ${alg} split equal:`, direct === split);
}
