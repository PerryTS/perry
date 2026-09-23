// #11046: `crypto.getHashes()` / `getCiphers()` / `getCurves()` /
// `getCipherInfo()` reached through a receiver the compiler cannot prove is
// `node:crypto`. undici's subresource-integrity module does exactly this at
// module init:
//
//   let crypto
//   if (runtimeFeatures.has('crypto')) {
//     crypto = require('node:crypto')
//     const cryptoHashes = crypto.getHashes()
//     if (cryptoHashes.length === 0) { ... }
//
// The statically-known `NativeModuleRef` form lowers straight to the runtime
// helper, but the dynamic form goes through the runtime's native-module
// dispatcher, which had no arm for these names and returned `undefined`, so
// `cryptoHashes.length` threw "Cannot read properties of undefined (reading
// 'length')" and every `require('undici')` failed.

// 1. undici's exact shape: a `let` assigned from require, then a feature probe.
let crypto: any;
crypto = require("node:crypto");
const cryptoHashes = crypto.getHashes();
console.log("let getHashes:", Array.isArray(cryptoHashes), cryptoHashes.length > 0);
const tokens = new Map([["sha256", 0], ["sha384", 1], ["sha512", 2]]);
for (const algorithm of tokens.keys()) {
  if (cryptoHashes.includes(algorithm) === false) {
    tokens.delete(algorithm);
  }
}
console.log("supported SRI algorithms:", [...tokens.keys()].join(","));

// 2. The other inventories through the same dynamic receiver.
const ciphers = crypto.getCiphers();
console.log("getCiphers:", Array.isArray(ciphers), ciphers.includes("aes-256-gcm"), ciphers.includes("aes-128-cbc"));
const curves = crypto.getCurves();
console.log("getCurves:", Array.isArray(curves), curves.includes("prime256v1"), curves.includes("secp384r1"));
const info = crypto.getCipherInfo("aes-256-cbc");
console.log("getCipherInfo:", info.name, info.mode, info.keyLength, info.ivLength, info.blockSize);
console.log("getCipherInfo unknown:", crypto.getCipherInfo("no-such-cipher"));

// 3. Detached method values and a receiver held in an object.
const getHashes = crypto.getHashes;
console.log("detached:", typeof getHashes, Array.isArray(getHashes()), getHashes().includes("sha1"));
const holder: any = { c: require("node:crypto") };
console.log("held:", Array.isArray(holder.c.getHashes()), holder.c.getCurves().length > 0);

// 4. Control: the statically-known receiver (already worked).
const direct = require("node:crypto");
console.log("const receiver:", Array.isArray(direct.getHashes()), direct.getHashes().includes("sha384"));
