// Behavioral parity test for the `argon2` npm wrapper.
//
// Perry routes `import argon2 from "argon2"` to perry-ext-argon2. Node
// would need the npm package installed; we use the expected-output
// mechanism instead.
//
// argon2.hash() embeds a random salt, so the literal hash output is
// non-deterministic. We assert by shape (`$argon2id$` prefix) and by
// round-trip argon2.verify against a freshly-produced hash. Only the
// async path is wired in Perry's dispatch
// (see crates/perry-codegen/src/lower_call.rs: `argon2.hash` / `verify`).
//
// @covers
// crates/perry-stdlib/src/argon2.rs:
//   - js_argon2_hash
//   - js_argon2_verify

import argon2 from "argon2";

async function main() {
  const hash = await argon2.hash("perry-parity");
  console.log("hash starts $argon2id$:", hash.startsWith("$argon2id$"));
  console.log("hash typeof:", typeof hash);

  const ok = await argon2.verify(hash, "perry-parity");
  console.log("verify correct:", ok);

  const bad = await argon2.verify(hash, "wrong-password");
  console.log("verify wrong:", bad);

  // A second hash of the same password produces a different hash (salt
  // changes) but still verifies. This confirms verify isn't just a
  // string-equality check.
  const hash2 = await argon2.hash("perry-parity");
  console.log("two hashes differ:", hash !== hash2);
  console.log("verify second:", await argon2.verify(hash2, "perry-parity"));
}

await main();
console.log("argon2 parity: ok");
