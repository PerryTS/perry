// Issue #925: compiler-rejected APIs should include a replacement hint
// in the error message so users find the supported form without grepping
// `perry --print-api-manifest`.
//
// This file is NOT meant to compile cleanly — each block triggers one of
// the error-emission paths the fix targets. The Rust-side unit tests in
// `crates/perry-hir/src/lower/unimpl_hints.rs` assert the actual hint
// payload; this file documents the user-visible shapes for posterity.
//
// To verify by hand:
//   1. Uncomment one block at a time.
//   2. Run `./target/release/perry test-files/test_issue_925_error_message_replacement.ts -o /tmp/x`.
//   3. Confirm the error contains the trailing "Use ..."/"is in/not in Perry's stdlib" hint.

// --- Case 1: crypto.hmacSha256 (2-deep `mod.method()` shape) ---
// Expected: error includes
//   "Use `crypto.createHmac(\"sha256\", key).update(data).digest(\"hex\")`"
//
// import * as crypto from "node:crypto";
// const sig = (crypto as any).hmacSha256("data", "key");

// --- Case 2a: require("crypto") used inline ---
// (`const x = require("crypto")` is silently rewritten to an ESM import
//  by a pre-existing path, so to hit the CJS-rejection gate the require
//  must appear in expression position.)
// Expected: error includes
//   "`crypto` is in Perry's stdlib — switch the `require` call to a static ESM import"
//
// const hmac = require("crypto").createHmac("sha256", "key");

// --- Case 2b: require("jose") (not in stdlib) ---
// Expected: error includes
//   "`jose` is not in Perry's stdlib"
//   "every method call will be `undefined` at runtime"
//
// const jose = require("jose");
// console.log(jose);

export {};
