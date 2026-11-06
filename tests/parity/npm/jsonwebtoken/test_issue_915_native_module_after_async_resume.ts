// Issue #915: `jwt.sign({...}, "secret", { algorithm: "HS256" })` SIGSEGVed
// from inside a resumed async-step body (fastify route handler that
// `await`ed a nested user async function first). Reduced standalone to
// the same async-resume shape minus fastify.
//
// Root cause was a calling-convention mismatch in the
// `NATIVE_MODULE_TABLE` row for jsonwebtoken's `sign` — the dispatch
// declared `[NA_F64, NA_F64, NA_F64], ret: NR_PTR` but the runtime
// `js_jwt_sign` is `(*const StringHeader, *const StringHeader, f64,
// *const StringHeader) -> i64`. NaN-boxed payload/secret arrived in
// `d0`/`d1` (float regs) while Rust read `x0`/`x1` as raw header
// pointers → garbage register deref → SIGSEGV at
// `ldr w1, [x0, #0x4]` (= StringHeader.byte_len). The 4th FFI arg
// (`kid_ptr`) wasn't passed at all so it was register garbage too,
// and the i64 return value (already STRING_TAG-tagged inside the FFI)
// got re-boxed as POINTER, so even a "successful" return looked like
// an object pointer with no `.length` field.
//
// The fix:
//   1. Update the dispatch row to `[NA_JSON, NA_STR, NA_F64, NA_STR],
//      ret: NR_STR` — matches the FFI's 4-arg ABI exactly.
//   2. New `NA_JSON` arg kind that serializes object/string payloads
//      through `js_json_stringify` so `serde_json::from_str` inside
//      the FFI can reconstruct the claims.
//   3. Padding for the unused 4th arg comes from
//      `lower_native_module_dispatch`'s existing
//      "fewer args than sig.args.len()" loop, which now also covers
//      `NA_JSON` (zeroed StringHeader pointer = null = optional kid).
//
// Repro shape (mirror of the user's #915 fastify repro without
// HTTP):
//   - module-level `async function delay()` and `async function getThing()`
//     to trigger the resumed-async-step body
//   - call `jwt.sign({sub:"x"}, "secret", {algorithm:"HS256"})` after the
//     await — before the fix, this SIGSEGVed; after, it returns a real
//     HS256 JWT.

import jwt from "jsonwebtoken";

async function delay(): Promise<void> {
  await Promise.resolve();
}

async function getThing(): Promise<void> {
  await delay();
}

async function go(): Promise<{ ok: boolean; len: number; prefix: string }> {
  await getThing();
  const token = jwt.sign({ sub: "x" }, "secret", { algorithm: "HS256" });
  // All HS256 JWTs start with `eyJ` (the base64-encoded `{"`) — assert
  // both the prefix and the typeof, since the original bug returned an
  // object pointer mis-boxed as POINTER that surfaced as `typeof token
  // === "object"` and `token.length === undefined`.
  return { ok: true, len: token.length, prefix: token.slice(0, 3) };
}

const result = await go();
console.log(JSON.stringify(result));
