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

// Exported bytes must be a detached Node Buffer, not a Uint8Array or key view.
const exported = secret.export();
console.log("secret export Buffer:", Buffer.isBuffer(exported));
console.log("secret export bytes:", exported.length, exported[0], exported[15]);
exported[0] = 255;
console.log("secret export copy independence:", secret.export()[0] === 0);
console.log("plain Uint8Array toString:", new Uint8Array([0, 17, 255]).toString());

// Serialized asymmetric exports must not inherit their source key's brand.
function checkExportIdentity() {
  const bytes = Buffer.from([0, 17, 34, 255]);
  const key = crypto.createSecretKey(bytes);
  const pair = crypto.generateKeyPairSync("ed25519");
  console.log("identity.secret", key instanceof KeyObject);
  console.log("identity.private", pair.privateKey instanceof KeyObject);
  console.log("identity.public", pair.publicKey instanceof KeyObject);
  console.log("identity.string", "ordinary" instanceof (KeyObject as any));
  const publicExport = pair.publicKey.export({ format: "pem", type: "spki" });
  const privateExport = pair.privateKey.export({ format: "pem", type: "pkcs8" });
  console.log("identity.pem", publicExport instanceof (KeyObject as any));
  console.log("identity.private.pem", privateExport instanceof (KeyObject as any));
  console.log("export.asymmetric.types", typeof publicExport, typeof privateExport);
  console.log("identity.public.after.export", pair.publicKey instanceof KeyObject);
  console.log("identity.private.after.export", pair.privateKey instanceof KeyObject);
  console.log(
    "export.public.bytes.stable",
    publicExport === pair.publicKey.export({ format: "pem", type: "spki" }),
  );
  console.log(
    "export.private.bytes.stable",
    privateExport === pair.privateKey.export({ format: "pem", type: "pkcs8" }),
  );
  console.log("identity.typedarray", new Uint8Array([0, 17]) instanceof KeyObject);
  console.log("identity.buffer", bytes instanceof KeyObject);
  console.log("identity.plain", { type: "secret" } instanceof KeyObject);
  const exported = key.export();
  console.log("export.buffer", Buffer.isBuffer(exported));
  console.log("export.typedarray", exported instanceof Uint8Array);
  console.log("export.not.key", exported instanceof KeyObject);
  console.log("export.value", exported.toString("hex"));
  const jwk: any = key.export({ format: "jwk" });
  console.log("export.jwk", jwk.kty, jwk.k);
  exported[0] = 99;
  bytes[1] = 88;
  jwk.k = "mutated";
  console.log("export.detached", key.export().toString("hex"));
  console.log("export.jwk.detached", (key.export({ format: "jwk" }) as any).k);
  console.log("typedarray.string", new Uint8Array([0, 17, 255]).toString());
  const ownHasInstance = Object.getOwnPropertyDescriptor(KeyObject, Symbol.hasInstance);
  try {
    Object.defineProperty(KeyObject, Symbol.hasInstance, {
      configurable: true,
      value: (value: any) => value === "custom",
    });
    console.log("hook.custom", "custom" instanceof (KeyObject as any));
    console.log("hook.secret", key instanceof KeyObject);
    console.log("hook.asymmetric", pair.publicKey instanceof KeyObject);
  } finally {
    if (ownHasInstance) {
      Object.defineProperty(KeyObject, Symbol.hasInstance, ownHasInstance);
    } else {
      delete (KeyObject as any)[Symbol.hasInstance];
    }
  }
}

checkExportIdentity();
