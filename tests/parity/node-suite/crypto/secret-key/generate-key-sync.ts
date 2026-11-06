import * as crypto from "node:crypto";

for (const length of [128, 192, 256]) {
  const key = crypto.generateKeySync("aes", { length });
  console.log("aes generated:", length, key.type, key.symmetricKeySize, key.export().length);
  console.log("aes generated jwk:", key.export({ format: "jwk" }).kty, !!key.export({ format: "jwk" }).k);
}

const hmac = crypto.generateKeySync("hmac", { length: 123 });
console.log("hmac generated:", hmac.type, hmac.symmetricKeySize, hmac.export().length);
console.log("hmac digest len:", crypto.createHmac("sha256", hmac).update("abc").digest().length);
console.log("hmac equals self copy:", hmac.equals(crypto.createSecretKey(hmac.export())));
