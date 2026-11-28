import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const SPKI_3072_E3 =
  "MIIBoDANBgkqhkiG9w0BAQEFAAOCAY0AMIIBiAKCAYEAzO++la6qiiKZmWgdKq2715MAEyQITFl2zajs5J4Ym2fKfIXpURoAhoap" +
  "XqhLbRWo/4JLWSG5ADP5rpJOMFkJw1Ru5VxHqo9M/0gMoVyfPCJhqatZZbypM9wVhB3vucbBn6uYAB66kZsQ8G6nfeI/NFOQo9L0" +
  "SOz6YPnGjYH8q00y/M0zFu9J8C9rMmZpVItE+fEGZGLxlm60VSuuZiMH6/KojNoBF94Mw/cnYboxUvLLKOS0n4M4ZpvSsjknbjjP" +
  "gPl2LBrwXRW+3dpz7Bp3g1wCa4X7nRmKnHfllf9g1NcmmAqnvCWIIUnvoFYZmlJoZFpD5b7hdjJib1uuWOLSTq0VHVk84bNLsc7a" +
  "g0yMnbkgs1E5ZvRlDoZCIDKPrWCDCZ/MKQ72jKLSlpnBcEY3/ZagTXoogvBFKoUWpL1fzHVaySq3udEUA22BRSRPl4rCq/1lpUmO" +
  "cd8rzMfx2UFIpCUZeD5Eiubcekxf0agUimZKt0o55D4RsPLBln+MXPSDAgED";

function describe(label: string, key: CryptoKey) {
  const algorithm = key.algorithm as RsaHashedKeyAlgorithm;
  console.log(
    label,
    key.type,
    Object.keys(algorithm).join(","),
    algorithm.modulusLength,
    algorithm.publicExponent instanceof Uint8Array,
    Array.from(algorithm.publicExponent ?? []).join(","),
    algorithm.hash.name,
  );
}

async function main() {
  for (const [name, usages] of [
    ["RSASSA-PKCS1-v1_5", ["sign", "verify"]],
    ["RSA-PSS", ["sign", "verify"]],
    ["RSA-OAEP", ["encrypt", "decrypt"]],
  ] as const) {
    const pair = await crypto.subtle.generateKey(
      { name, modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
      true,
      [...usages],
    );
    describe(`${name} generated`, pair.privateKey);
    describe(`${name} generated`, pair.publicKey);

    const [privateUsage, publicUsage] = name === "RSA-OAEP" ? ["decrypt", "encrypt"] as const : ["sign", "verify"] as const;
    const algorithm = { name, hash: "SHA-384" };
    for (const format of ["jwk", "spki"] as const) {
      const data: any = await crypto.subtle.exportKey(format, pair.publicKey);
      if (format === "jwk") delete data.alg;
      describe(`${name} ${format}`, await crypto.subtle.importKey(format as any, data, algorithm, true, [publicUsage]));
    }
    for (const format of ["jwk", "pkcs8"] as const) {
      const data: any = await crypto.subtle.exportKey(format, pair.privateKey);
      if (format === "jwk") delete data.alg;
      describe(`${name} ${format}`, await crypto.subtle.importKey(format as any, data, algorithm, true, [privateUsage]));
    }
  }

  const spki = Buffer.from(SPKI_3072_E3, "base64");
  const imported = await crypto.subtle.importKey(
    "spki",
    spki,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-512" },
    true,
    ["verify"],
  );
  describe("3072 e=3", imported);

  const { publicKey } = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
  const converted = (publicKey as any).toCryptoKey({ name: "RSA-PSS", hash: "SHA-256" }, true, ["verify"]);
  describe("toCryptoKey", converted);
}
await main();
