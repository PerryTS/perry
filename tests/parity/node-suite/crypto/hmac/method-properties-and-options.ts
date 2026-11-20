import * as crypto from "node:crypto";

const hmac = crypto.createHmac("sha256", "key");
console.log("hmac update name:", hmac.update.name);
console.log("hmac digest name:", hmac.digest.name);
console.log("hmac update typeof:", typeof hmac.update);
console.log("hmac digest typeof:", typeof hmac.digest);

for (const encoding of [undefined, null, "utf8", "utf-8", "ascii", "binary", "hex", "base64", "base64url"] as any[]) {
  const h = crypto.createHmac("sha256", "a secret", { encoding } as any);
  h.update("some data to hash");
  console.log("hmac option len", String(encoding), h.digest("hex").length);
}
