import { createRequire } from "node:module";
import * as nodeCrypto from "node:crypto";
import { argon2, argon2Sync, createHash, pbkdf2Sync } from "node:crypto";

// Importing node:module routes named node:crypto imports through the dynamic
// dispatcher instead of the static call lowering.
void createRequire;

const enc = new TextEncoder();
const iterations = Number("4096");

const derived = pbkdf2Sync(enc.encode("password"), enc.encode("salt"), iterations, 32, "sha256");
console.log("pbkdf2Sync", typeof derived, derived.length, derived.toString("hex"));
console.log("pbkdf2Sync bytes", Uint8Array.from(derived).length);

const fromStrings = pbkdf2Sync("password", "salt", iterations, 16, "sha1");
console.log("pbkdf2Sync strings", typeof fromStrings, fromStrings.length, fromStrings.toString("hex"));

const viaNamespace = nodeCrypto.pbkdf2Sync(enc.encode("password"), enc.encode("salt"), iterations, 32, "sha256");
console.log("pbkdf2Sync matches namespace", derived.equals(viaNamespace));

try {
  pbkdf2Sync("password", "salt", 0, 32, "sha256");
} catch (error: any) {
  console.log("pbkdf2Sync iterations", error.code);
}
try {
  (pbkdf2Sync as any)("password", "salt", 1, 32, 256);
} catch (error: any) {
  console.log("pbkdf2Sync digest", error.code);
}

const argonParams = {
  message: Buffer.from("password"),
  nonce: Buffer.from("0123456789abcdef"),
  parallelism: 1,
  tagLength: 16,
  memory: 8,
  passes: 1,
};
const argonKey = argon2Sync("argon2id", argonParams);
console.log("argon2Sync", typeof argonKey, argonKey.length, argonKey.toString("hex"));

argon2("argon2id", argonParams, (error, key) => {
  console.log("argon2", error, key.length, key.toString("hex"));
});

console.log("createHash", createHash("sha256").update("x").digest("hex"));
