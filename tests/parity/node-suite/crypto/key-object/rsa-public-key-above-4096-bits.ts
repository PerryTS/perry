import { Buffer } from "node:buffer";
import * as crypto from "node:crypto";

// A 5120-bit key and its signatures over "large rsa key".
const SPKI =
  "MIICojANBgkqhkiG9w0BAQEFAAOCAo8AMIICigKCAoEAt6c8CFqIwjBfY142cU6NQlP6MsQeKw2eBnt4au01qwk/u0sBb72QH7SLsmz77lBeJB" +
  "frxbk3Na7Bx3ZZJirJWVgT4ymbfc0cgytRJIlcySrNXoftZ/ymgZH5McmTdOxwjuFS06Es7Pf5lYrLQDdv9D4GnqE9KrTV9iB6HMXuU/2dslQS" +
  "Qo2G+kprYkuRn25c5lMATK2knoW5tPwOjXmh0Bb4a4HOJnoRNrFQVXlBdoiOUCFxGqld4GnR7i/PELu72EtG0zzdWrVhjTkbkTY4hkfsclRccz" +
  "BmM+ryfWWpni/IYfNQDR+/57ibdaaWFoiqSNSdXqAOkYwD3WcKyvSui/qnmBHmScrn6z0yoHKui0pYj6xMJ3FYNDGZruaT+1J/EKkJVaFdcSFC" +
  "+PcBT4VLnDXfYo4LqLHFquhsrDi58YfRS/4OjdX9GoR71q9LqON3hy4gJmtb3Wt7BWoF7b0/1JAGr44O1t34a6LmK3Q+MLuhdxY7+8BG+BbCRz" +
  "4IDyxmYImxzvkBr4pUqTdMlnmGq2Ii1QcaCbTLVDQj13eEHWHX8luTcueSQnMPu1jmurzeEG0vyWx9g2ycTO0mvnWWAz+MFrxAnbPNeFsJskjg" +
  "aDVrmQxDl9EGaRu6YwJM7sdhg0CfZWGaDAZ6+hh7sWI96EDqNCRO6FwbioTiKKrrA+tsqDXkmYz7E3YgmXGoXGIWo11PimBSb8WDhYjvs6QEjJ" +
  "lgXcs8UCseu12U+yRIAr8mdDMZ+xNJCQcoS9MKXU+EPSJy4ETAZGVP0lK7LTUdr9Dirq/fRJA0osCkea7RBujUsa+IkpHzlybdDZuQ+2AmaAnw" +
  "ycJEN9tnXmv1Yi3p8QIDAQAB";
const PKCS1_SIGNATURE =
  "Gqn6J06LCJ9JIY1CSq0Qg+cUG/mXa7TMI34Z4acO/SqF8yc2aP6KSQGAJYk+4GDlbKwqOevMK/9XOhLaisE4RZL5YeiRARFzXs9673tEOH5K9W" +
  "dK5QUCxQRNIzetoYz38WmCAkLkWCT3BUwwc2I2twr1DMjf5je3NCTF4nuGZwoSwAfFWusCRW+g2bvHsEfleCbi3QfXxtySmBMbuFjuF/bJZvOp" +
  "ax3nZkphwBSbbCPaBa/VAzEdem1yue+k/3JtaDzKd8A4tPU1dSrOgw7wLEgKsNFf7Thf9ZxTaFlP5NQDmFMLb7xCAS9KPJy+DEFHBOpiCIBL1z" +
  "bkWB1clRJPv9FgyVUbfAntqqYKZEc0+bosqenqkRazpvXmShLY0KDBiRJZddluYP/bt6lHX9Ecq+nUZnCcV9/JivSdv+4CjGL06pM4y/8HEyU4" +
  "chXBS0WybL+kpqM6xfYPKyT7l0GMBcgoiekqg9DW9Dljd6teIfw9PmU4ju4cA2jWmudAMT5qpuB+rdSV8OJQEH52O/ZnjVb+nIhMR5m/RB0Y3y" +
  "QoQW31ykcJOAJ2ifcpQtwHtN06ghcwyhpR1CNK4j+9htA2bU6GBOka0EBsLhL+NdIjPJkbOJmJ+wwAI9MYtDywf0Z6ipwM3x1vd1DkC5jifvsC" +
  "OGwCzp5pyal3Tx5mkc3xGuRtvEhvm7Hf/2jPdQliBa5YJlrKf1t8PjELYKm9GbsKAHCkVejfcRGYV5WktzgD1ftuKqYOOpO311UJPAp2LHRG5o" +
  "CBzZ4Gs+9yWZkin2WS4GBCL6H3rDMLkVrQKxWQxv+4qJhJ8aI+o9ZLsNMpANfdh1uiTKU2te/eUnFwv1XP+A==";
const PSS_SIGNATURE =
  "r/jMtp1T//daWAzCwhJoi/pdr/ZQ0UzCiAutcufhm8Blv8lM0tWiBt6fgBz9owqIHzKAWJsghSXlpC3hW72TsjUHObEZaJLhK9qYFN426yLToF" +
  "Kj/hdJVnYmFGFolAJ/hDNKrbk05RClkOxj4YD2Qv9Tc47YwDVdZCdN/LfY2SSz0nSIoAOD5OeiyrXB42uLGIWfsFkxng6W3csb4nhZj9VRm8yL" +
  "JL8V6sPYsWYv7Hbn94f3u6CrGZYgF0vlA7w8mcioUhFExLS4vqtOFQKJF0soFndQYky3nMCKCxHFQkHAqS8dsyJI4j6XcVUiYQ55HRCvpU4fq2" +
  "el1KdXr1JLLnI5kOg7jTgTmJRywfw3R+tf8nF2kuSPYNLdMdg6XzmjwBdU0HTNNyrYUmE13+SZQuAC9+qfBv9RZ1CDSMR+71TLppcTTNiVaHrD" +
  "oorFw/Kc6FzTSOxiaW5ePPrXnI/jnLZW/ojuC8LcUrpwQfrKCPRmfei1waNVat/p87DY5MbtuTe3SlbEuG+qbTFfrf+r8SSEYP+eA+sWTzGvVK" +
  "JAz9QhwgtBB1BuyMREylXWQYyglFlMWuv9sNzb1G47trzNwLU0hdVnyMHPbCRHr5JgVYnRgW0cFaQLWijeFZtOxwmh7iASmgPdoYro0vP7JYC7" +
  "S+F3uypmRi5T2KTqSLGm5qZEwI68s+T02UekzJWPtssUeYosAUTQugOqnaUvOHOZHmlNk1Arvyy9qKrVyzPb64dIQpM+6sQmQ28Q4jcNXjwSlA" +
  "FhxqshZz99vdhRQCOHGjepxaSeUF1xwiWWBNk8sKv3JVe7i/7moXxTv5IwzYxaGL7SZs9TeAEkrmCB/r7SUg==";

const der = Buffer.from(SPKI, "base64");
const pem = `-----BEGIN PUBLIC KEY-----\n${SPKI.match(/.{1,64}/g)!.join("\n")}\n-----END PUBLIC KEY-----\n`;
const data = Buffer.from("large rsa key");
const pkcs1 = Buffer.from(PKCS1_SIGNATURE, "base64");
const pss = Buffer.from(PSS_SIGNATURE, "base64");

console.log("pem verify", crypto.verify("sha256", data, pem, pkcs1));
console.log("pem verify, other data", crypto.verify("sha256", Buffer.from("other"), pem, pkcs1));
const key = crypto.createPublicKey(pem);
console.log("key verify", crypto.verify("sha256", data, key, pkcs1));
console.log("encrypt", crypto.publicEncrypt(pem, data).length);
const jwk: any = key.export({ format: "jwk" });
console.log("jwk verify", crypto.verify("sha256", data, { key: jwk, format: "jwk" }, pkcs1));

const algorithm = { name: "RSA-PSS", hash: "SHA-256" };
const spkiKey = await crypto.subtle.importKey("spki", der, algorithm, true, ["verify"]);
console.log("subtle pss", await crypto.subtle.verify({ name: "RSA-PSS", saltLength: 32 }, spkiKey, pss, data));
const exported: any = await crypto.subtle.exportKey("jwk", spkiKey);
console.log("subtle jwk", exported.n === jwk.n, exported.e);
const jwkKey = await crypto.subtle.importKey("jwk", { kty: "RSA", n: jwk.n, e: jwk.e }, { ...algorithm, name: "RSASSA-PKCS1-v1_5" }, true, ["verify"]);
console.log("subtle pkcs1", await crypto.subtle.verify("RSASSA-PKCS1-v1_5", jwkKey, pkcs1, data));
const oaep = await crypto.subtle.importKey("spki", der, { name: "RSA-OAEP", hash: "SHA-256" }, true, ["encrypt", "wrapKey"]);
console.log("subtle encrypt", (await crypto.subtle.encrypt("RSA-OAEP", oaep, data)).byteLength);
const aes = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt"]);
console.log("subtle wrap", (await crypto.subtle.wrapKey("raw", aes, oaep, "RSA-OAEP")).byteLength);
