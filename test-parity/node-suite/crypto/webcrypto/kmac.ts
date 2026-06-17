import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

(process as any).emitWarning = () => undefined;

const data = new TextEncoder().encode("kmac payload");

function b64u(bytes: number[]) {
  return Buffer.from(bytes).toString("base64url");
}

async function logReject(label: string, promise: Promise<unknown>) {
  let rejected = false;
  let name = "";
  try {
    await promise;
  } catch (error: any) {
    rejected = true;
    name = error?.name ?? "";
  }
  console.log(`${label}:`, rejected, name);
}

async function logKmac(label: string, algorithm: any, key: CryptoKey, payload = data) {
  const sig = await crypto.subtle.sign(algorithm, key, payload);
  console.log(`${label} len:`, Buffer.from(sig).length);
  console.log(`${label} hex:`, Buffer.from(sig).toString("hex"));
  console.log(`${label} verify ok:`, await crypto.subtle.verify(algorithm, key, sig, payload));
  console.log(
    `${label} verify bad data:`,
    await crypto.subtle.verify(algorithm, key, sig, new TextEncoder().encode("bad payload")),
  );
  return sig;
}

async function main() {
  for (const op of ["generateKey", "importKey", "exportKey", "sign", "verify"] as const) {
    console.log(`supports ${op} KMAC128:`, SubtleCrypto.supports(op, "KMAC128"));
    console.log(`supports ${op} KMAC256:`, SubtleCrypto.supports(op, "KMAC256"));
  }

  const generated128 = await crypto.subtle.generateKey({ name: "KMAC128", length: 16 }, true, ["sign", "verify"]);
  console.log(
    "generated128:",
    generated128.type,
    generated128.algorithm.name,
    (generated128.algorithm as any).length,
    generated128.extractable,
    generated128.usages.join(","),
  );

  const generated256 = await crypto.subtle.generateKey("KMAC256", false, ["sign"]);
  console.log(
    "generated256:",
    generated256.type,
    generated256.algorithm.name,
    (generated256.algorithm as any).length,
    generated256.extractable,
    generated256.usages.join(","),
  );

  const jwk128 = {
    kty: "oct",
    alg: "K128",
    k: b64u([1, 2, 3, 4, 5, 6, 7, 8]),
    ext: true,
    key_ops: ["sign", "verify"],
  };
  const key128 = await crypto.subtle.importKey("jwk", jwk128, "KMAC128", true, ["sign", "verify"]);
  const exported128: any = await crypto.subtle.exportKey("jwk", key128);
  console.log("jwk128 key:", key128.type, key128.algorithm.name, (key128.algorithm as any).length, key128.usages.join(","));
  console.log("jwk128 export:", exported128.kty, exported128.alg, exported128.k === jwk128.k);
  await logKmac("kmac128", { name: "KMAC128", outputLength: 128 }, key128);
  await logKmac("kmac128 custom", { name: "KMAC128", outputLength: 64, customization: new Uint8Array([1, 2, 3]) }, key128);

  const jwk256 = {
    kty: "oct",
    alg: "K256",
    k: b64u([9, 8, 7, 6, 5, 4, 3, 2, 1]),
    ext: true,
    key_ops: ["sign", "verify"],
  };
  const key256 = await crypto.subtle.importKey("jwk", jwk256, "KMAC256", true, ["sign", "verify"]);
  const exported256: any = await crypto.subtle.exportKey("jwk", key256);
  console.log("jwk256 key:", key256.type, key256.algorithm.name, (key256.algorithm as any).length, key256.usages.join(","));
  console.log("jwk256 export:", exported256.kty, exported256.alg, exported256.k === jwk256.k);
  await logKmac("kmac256", { name: "KMAC256", outputLength: 256 }, key256);
  await logKmac("kmac256 custom", { name: "KMAC256", outputLength: 128, customization: new Uint8Array([4, 5]) }, key256);

  const emptySig = await crypto.subtle.sign({ name: "KMAC128", outputLength: 0 }, key128, data);
  console.log("kmac128 zero len:", Buffer.from(emptySig).length);
  console.log("kmac128 zero verify:", await crypto.subtle.verify({ name: "KMAC128", outputLength: 0 }, key128, emptySig, data));

  await logReject("kmac128 raw import", crypto.subtle.importKey("raw", new Uint8Array(8), "KMAC128", true, ["sign"]));
  await logReject("kmac256 raw import", crypto.subtle.importKey("raw", new Uint8Array(8), "KMAC256", true, ["sign"]));
  await logReject("kmac128 raw export", crypto.subtle.exportKey("raw", key128));
  await logReject("kmac128 empty usages", crypto.subtle.generateKey("KMAC128", true, []));
  await logReject("kmac128 bad usage", crypto.subtle.generateKey("KMAC128", true, ["encrypt" as any]));
  await logReject("kmac128 bad key length", crypto.subtle.generateKey({ name: "KMAC128", length: 7 }, true, ["sign"]));
  await logReject("kmac128 zero key", crypto.subtle.importKey("jwk", { kty: "oct", alg: "K128", k: "", ext: true, key_ops: ["sign"] }, "KMAC128", true, ["sign"]));
  await logReject("kmac128 bad output", crypto.subtle.sign({ name: "KMAC128", outputLength: 7 }, key128, data));
}

await main();
