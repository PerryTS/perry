// A CommonJS module destructures named `node:crypto` exports from `require`
// instead of using the static ESM call lowering, so every call below goes
// through `js_crypto_native_dispatch`. That dispatcher had no arms for
// `pbkdf2Sync`, `argon2Sync` or `argon2`: they returned `undefined` (and the
// callback form never called back) while the namespace call kept working.
// This is the shape a compiled CJS dependency of the Supabase CLI uses for
// SCRAM-SHA-256 authentication.
const { argon2, argon2Sync, pbkdf2Sync } = require("node:crypto");
const nodeCrypto = require("node:crypto");

const iterations = Number("4096");

// String password/salt take the string branch of the dispatcher's argument.
const key = pbkdf2Sync("password", "salt", iterations, 32, "sha256");
console.log("pbkdf2Sync strings", typeof key, key.length, key.toString("hex"));
console.log(
  "pbkdf2Sync matches namespace",
  key.equals(nodeCrypto.pbkdf2Sync("password", "salt", iterations, 32, "sha256")),
);

// Uint8Array password/salt take the buffer branch of the same argument.
const typed = pbkdf2Sync(
  new TextEncoder().encode("password"),
  new TextEncoder().encode("salt"),
  iterations,
  16,
  "sha1",
);
console.log("pbkdf2Sync typed", typed.length, typed.toString("hex"), Uint8Array.from(typed).length);

// Node validates both integers and the digest before deriving; the omitted
// digest is an error, not a SHA-256 default.
for (const [label, call] of [
  ["iterations 0", () => pbkdf2Sync("password", "salt", 0, 32, "sha256")],
  ["digest not a string", () => pbkdf2Sync("password", "salt", 1, 32, 256)],
  ["digest omitted", () => pbkdf2Sync("password", "salt", 1, 32)],
  ["digest unknown", () => pbkdf2Sync("password", "salt", 1, 32, "not-a-digest")],
] as [string, () => unknown][]) {
  try {
    call();
    console.log(label, "no throw");
  } catch (error: any) {
    console.log(label, error.code);
  }
}

const params = {
  message: Buffer.from("password"),
  nonce: Buffer.from("0123456789abcdef"),
  parallelism: 1,
  tagLength: 16,
  memory: 8,
  passes: 1,
};
const argonKey = argon2Sync("argon2id", params);
console.log("argon2Sync", typeof argonKey, argonKey.length, argonKey.toString("hex"));

argon2("argon2id", params, (error: any, callbackKey: any) => {
  if (error) {
    console.log("argon2 error", error);
    return;
  }
  console.log("argon2", callbackKey.length, callbackKey.toString("hex"));
});
