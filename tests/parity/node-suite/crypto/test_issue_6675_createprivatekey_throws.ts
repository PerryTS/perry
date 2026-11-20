// Issue #6675: jsonwebtoken jwt.sign(payload, "<string secret>", {algorithm:"HS256"})
// threw "Cannot read properties of undefined (reading 'type')" under Perry.
//
// jsonwebtoken's sign.js normalizes a string secret with:
//   if (secretOrPrivateKey != null && !(secretOrPrivateKey instanceof KeyObject)) {
//     try { secretOrPrivateKey = createPrivateKey(secretOrPrivateKey) }
//     catch (_) { try { secretOrPrivateKey = createSecretKey(Buffer.from(secretOrPrivateKey)) } catch (_) {...} }
//   }
//   if (header.alg.startsWith('HS') && secretOrPrivateKey.type !== 'secret') {...}
//
// In Node, createPrivateKey("<plain string>") THROWS (invalid key material), so
// the catch runs createSecretKey and produces a KeyObject with .type === "secret".
//
// The bundled jsonwebtoken reaches these constructors as VALUES (via
// require('crypto').createPrivateKey) rather than direct member calls, so it hits
// the runtime value-dispatch. That dispatch had no arms for the key constructors
// and returned undefined without throwing, so the catch never ran,
// secretOrPrivateKey became undefined, and reading `.type` crashed.
//
// This test mirrors that by obtaining the constructors through a value the
// compiler can't statically resolve to crypto.<method> — exercising the runtime
// value-dispatch path exactly like the bundled package does.
import * as cryptoNs from "crypto";

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

const crypto = cryptoNs as any;
const createPrivateKey = crypto["createPrivateKey"];
const createSecretKey = crypto["createSecretKey"];
const KeyObject = crypto["KeyObject"];

function normalizeSecret(secretOrPrivateKey: any): any {
  if (secretOrPrivateKey != null && !(secretOrPrivateKey instanceof KeyObject)) {
    try {
      secretOrPrivateKey = createPrivateKey(secretOrPrivateKey);
    } catch (_) {
      try {
        secretOrPrivateKey = createSecretKey(
          typeof secretOrPrivateKey === "string"
            ? Buffer.from(secretOrPrivateKey)
            : secretOrPrivateKey,
        );
      } catch (_) {
        throw new Error("secretOrPrivateKey is not valid key material");
      }
    }
  }
  return secretOrPrivateKey;
}

const key = normalizeSecret("secret-value-here");
// The console.log lines are the byte-for-byte parity oracle (diffed against
// `node --experimental-strip-types`); the asserts make the test fail loudly if
// it is ever run standalone with an incorrect result.
console.log("type:", key.type);
console.log("isSecret:", key.type === "secret");
assert(
  key.type === "secret",
  "normalizeSecret must fall back to a secret KeyObject",
);

// createPublicKey must also THROW on a plain string (the verify() path relies on
// this to fall through to createSecretKey for HS* tokens).
const createPublicKey = crypto["createPublicKey"];
let pubThrew = false;
try {
  createPublicKey("secret-value-here");
} catch (_) {
  pubThrew = true;
}
console.log("createPublicKey throws on plain string:", pubThrew);
assert(pubThrew, "createPublicKey must throw on a plain (non-key) string");
