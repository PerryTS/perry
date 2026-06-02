import * as crypto from "node:crypto";
import { KeyObject as NamedKeyObject } from "node:crypto";
import { Buffer } from "node:buffer";

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "");
  }
}

async function reportAsync(label: string, fn: () => Promise<unknown>) {
  try {
    console.log(`${label}:`, await fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "");
  }
}

const KeyObject = (crypto as any).KeyObject;
const secret = crypto.createSecretKey(Buffer.from("00112233445566778899aabbccddeeff", "hex"));

console.log("typeof KeyObject:", typeof KeyObject);
console.log("typeof KeyObject.from:", typeof (KeyObject && KeyObject.from));
console.log("KeyObject length:", KeyObject.length);
console.log("KeyObject.from length:", KeyObject.from.length);
console.log("KeyObject prototype object:", typeof KeyObject.prototype);
console.log("KeyObject prototype constructor:", KeyObject.prototype.constructor === KeyObject);
console.log("named KeyObject identity:", NamedKeyObject === KeyObject);
report("secret instanceof KeyObject", () => secret instanceof KeyObject);
console.log("secret type:", secret.type);
console.log("secret export hex:", secret.export().toString("hex"));

await reportAsync("from CryptoKey", async () => {
  const cryptoKey = await crypto.webcrypto.subtle.importKey(
    "raw",
    secret.export(),
    { name: "HMAC", hash: "SHA-256" },
    true,
    ["sign"],
  );
  const keyObject = KeyObject.from(cryptoKey);
  return `${keyObject.type} ${keyObject instanceof KeyObject} ${keyObject.export().toString("hex")}`;
});

report("from undefined", () => KeyObject.from(undefined));
report("from null", () => KeyObject.from(null));
report("from object", () => KeyObject.from({}));
report("from KeyObject", () => KeyObject.from(secret));
