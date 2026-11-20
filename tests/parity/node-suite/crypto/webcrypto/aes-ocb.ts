import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

(process as any).emitWarning = () => undefined;

async function logReject(label: string, promise: Promise<any>) {
  try {
    await promise;
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}:`, error.name);
  }
}

function algShape(key: CryptoKey) {
  return JSON.stringify(key.algorithm);
}

function usages(key: CryptoKey) {
  return key.usages.join(",");
}

async function main() {
  for (const op of ["generateKey", "importKey", "exportKey", "encrypt", "decrypt", "wrapKey", "unwrapKey"] as const) {
    console.log(`supports ${op}:`, SubtleCrypto.supports(op, "AES-OCB" as any));
  }

  const generated = await crypto.subtle.generateKey(
    { name: "AES-OCB" as any, length: 128 },
    true,
    ["encrypt", "decrypt", "wrapKey", "unwrapKey"],
  );
  console.log("generated type:", generated.type);
  console.log("generated extractable:", generated.extractable);
  console.log("generated algorithm:", algShape(generated));
  console.log("generated usages:", usages(generated));

  await logReject("generate string", crypto.subtle.generateKey("AES-OCB" as any, true, ["encrypt"]));
  await logReject("generate missing length", crypto.subtle.generateKey({ name: "AES-OCB" } as any, true, ["encrypt"]));
  await logReject("generate bad length", crypto.subtle.generateKey({ name: "AES-OCB" as any, length: 64 }, true, ["encrypt"]));
  await logReject("generate empty usages", crypto.subtle.generateKey({ name: "AES-OCB" as any, length: 128 }, true, []));
  await logReject("generate bad usage", crypto.subtle.generateKey({ name: "AES-OCB" as any, length: 128 }, true, ["sign" as any]));

  const keyBytes = Buffer.from("0102030405060708090a0b0c0d0e0f10", "hex");
  const jwk = { kty: "oct", k: keyBytes.toString("base64url") };
  const key = await crypto.subtle.importKey("jwk", jwk, "AES-OCB" as any, true, ["encrypt", "decrypt", "wrapKey", "unwrapKey"]);
  const keyWithAlg = await crypto.subtle.importKey(
    "jwk",
    { ...jwk, alg: "A128OCB" },
    "AES-OCB" as any,
    true,
    ["encrypt", "decrypt"],
  );
  console.log("import no alg algorithm:", algShape(key));
  console.log("import alg algorithm:", algShape(keyWithAlg));
  console.log("import alg usages:", usages(keyWithAlg));

  const exported = await crypto.subtle.exportKey("jwk", key) as JsonWebKey;
  console.log("export kty:", exported.kty);
  console.log("export alg:", exported.alg);
  console.log("export key roundtrip:", exported.k === jwk.k);

  await logReject("raw import", crypto.subtle.importKey("raw", keyBytes, "AES-OCB" as any, true, ["encrypt"]));
  await logReject("raw export", crypto.subtle.exportKey("raw", key));
  await logReject("jwk alg mismatch", crypto.subtle.importKey("jwk", { ...jwk, alg: "A192OCB" }, "AES-OCB" as any, true, ["encrypt"]));
  await logReject(
    "jwk invalid length",
    crypto.subtle.importKey("jwk", { kty: "oct", k: Buffer.from([1, 2, 3]).toString("base64url") }, "AES-OCB" as any, true, ["encrypt"]),
  );

  const data = new TextEncoder().encode("hello ocb");
  const iv = Buffer.alloc(12, 7);

  async function roundTrip(label: string, params: any) {
    const ct = Buffer.from(await crypto.subtle.encrypt(params, key, data));
    console.log(`${label} len:`, ct.length);
    console.log(`${label} hex:`, ct.toString("hex"));
    const pt = await crypto.subtle.decrypt(params, key, ct);
    console.log(`${label} pt:`, Buffer.from(pt).toString());
    return ct;
  }

  const defaultCt = await roundTrip("default", { name: "AES-OCB", iv });
  const aadCt = await roundTrip("aad", { name: "AES-OCB", iv, additionalData: new Uint8Array([1, 2]) });
  await roundTrip("tag96", { name: "AES-OCB", iv, tagLength: 96 });
  await roundTrip("tag64", { name: "AES-OCB", iv, tagLength: 64 });
  await roundTrip("iv1", { name: "AES-OCB", iv: Buffer.alloc(1, 7) });
  await roundTrip("iv5", { name: "AES-OCB", iv: Buffer.alloc(5, 7) });
  await roundTrip("iv15", { name: "AES-OCB", iv: Buffer.alloc(15, 7) });

  await logReject("wrong aad decrypt", crypto.subtle.decrypt({ name: "AES-OCB", iv, additionalData: new Uint8Array([2, 1]) } as any, key, aadCt));
  const wrongKey = await crypto.subtle.importKey(
    "jwk",
    { kty: "oct", k: Buffer.from("1112131415161718191a1b1c1d1e1f20", "hex").toString("base64url") },
    "AES-OCB" as any,
    true,
    ["decrypt"],
  );
  await logReject("wrong key decrypt", crypto.subtle.decrypt({ name: "AES-OCB", iv } as any, wrongKey, defaultCt));
  await logReject("iv empty encrypt", crypto.subtle.encrypt({ name: "AES-OCB", iv: new Uint8Array(0) } as any, key, data));
  await logReject("iv too long encrypt", crypto.subtle.encrypt({ name: "AES-OCB", iv: Buffer.alloc(16, 7) } as any, key, data));
  await logReject("bad tag length encrypt", crypto.subtle.encrypt({ name: "AES-OCB", iv, tagLength: 120 } as any, key, data));

  const target = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, ["encrypt", "decrypt"]);
  const wrapped = await crypto.subtle.wrapKey("raw", target, key, { name: "AES-OCB", iv } as any);
  console.log("wrap len:", wrapped.byteLength);
  const unwrapped = await crypto.subtle.unwrapKey(
    "raw",
    wrapped,
    key,
    { name: "AES-OCB", iv } as any,
    "AES-GCM",
    true,
    ["encrypt"],
  );
  console.log("unwrap algorithm:", algShape(unwrapped));
  console.log("unwrap usages:", usages(unwrapped));
}

await main();
