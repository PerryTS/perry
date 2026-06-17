import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

(process as any).emitWarning = () => {};

const password = new TextEncoder().encode("password");
const nonce = Uint8Array.from([1, 2, 3, 4, 5, 6, 7, 8]);

function hex(value: ArrayBuffer | Uint8Array): string {
  return Buffer.from(value instanceof Uint8Array ? value : new Uint8Array(value)).toString("hex");
}

async function reportError(label: string, fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn();
    console.log(`${label}: ok`);
  } catch (error: any) {
    console.log(`${label}: ${error.name}: ${error.message}`);
  }
}

async function runAlgorithm(name: "Argon2d" | "Argon2i" | "Argon2id"): Promise<void> {
  console.log(`${name} supports import:`, crypto.subtle.constructor.supports("importKey", name));
  console.log(`${name} supports deriveBits:`, crypto.subtle.constructor.supports("deriveBits", name));
  console.log(`${name} supports deriveKey:`, crypto.subtle.constructor.supports("deriveKey", name));

  const params = { name, nonce, memory: 8, passes: 1, parallelism: 1 };
  const key = await crypto.subtle.importKey("raw-secret", password, name, false, [
    "deriveBits",
    "deriveKey",
  ]);
  console.log(
    `${name} key:`,
    key.algorithm.name,
    key.type,
    key.extractable,
    key.usages.join(","),
  );

  const bits = await crypto.subtle.deriveBits(params, key, 128);
  console.log(`${name} bits128:`, hex(bits));

  const derivedKey = await crypto.subtle.deriveKey(
    params,
    key,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  console.log(
    `${name} derived key:`,
    derivedKey.algorithm.name,
    (derivedKey.algorithm as any).length,
    derivedKey.extractable,
    derivedKey.usages.join(","),
  );
  console.log(`${name} derived raw:`, hex(await crypto.subtle.exportKey("raw", derivedKey)));
}

await runAlgorithm("Argon2d");
await runAlgorithm("Argon2i");
await runAlgorithm("Argon2id");

await reportError("extractable argon2 key", () =>
  crypto.subtle.importKey("raw-secret", password, "Argon2id", true, ["deriveBits"]),
);
await reportError("invalid argon2 usage", () =>
  crypto.subtle.importKey("raw-secret", password, "Argon2id", false, ["encrypt"]),
);

const deriveOnlyKey = await crypto.subtle.importKey("raw-secret", password, "Argon2id", false, [
  "deriveKey",
]);
await reportError("deriveBits missing usage", () =>
  crypto.subtle.deriveBits(
    { name: "Argon2id", nonce, memory: 8, passes: 1, parallelism: 1 },
    deriveOnlyKey,
    128,
  ),
);

const bitsOnlyKey = await crypto.subtle.importKey("raw-secret", password, "Argon2id", false, [
  "deriveBits",
]);
await reportError("algorithm mismatch", () =>
  crypto.subtle.deriveBits(
    { name: "Argon2i", nonce, memory: 8, passes: 1, parallelism: 1 },
    bitsOnlyKey,
    128,
  ),
);
await reportError("short nonce", () =>
  crypto.subtle.deriveBits(
    { name: "Argon2id", nonce: new Uint8Array(7), memory: 8, passes: 1, parallelism: 1 },
    bitsOnlyKey,
    128,
  ),
);
await reportError("low memory", () =>
  crypto.subtle.deriveBits(
    { name: "Argon2id", nonce, memory: 7, passes: 1, parallelism: 1 },
    bitsOnlyKey,
    128,
  ),
);
