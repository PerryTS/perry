// Issue #63 / #321: `js_dyn_index_get` MUST NOT SIGBUS when its receiver
// is a number whose f64 bit pattern happens to satisfy the
// "looks-like-a-raw-I64-pointer" heuristic the helper falls back on for
// module-level objects stored as raw I64 (where LLVM ABI passes them as
// DOUBLE).
//
// Before the fix, effect's fiberRefs.ts loop produced a value whose
// f64 bits were a small denormal (~0x8_0000_0000, value ~1.7e-314).
// That value: not-NaN, non-zero, < 2^48, low-2 bits zero, >= 0x10000 —
// so the heuristic accepted it as a "raw pointer", read
// `(*gc_hdr).obj_type` at `[bits - 8]`, and SIGBUSed crossing the
// macOS user/kernel boundary.
//
// This test exercises the same dataflow shape (a denormal-bits number
// reaching `obj[idx]` with both operands typed Any/Unknown so codegen
// picks the runtime `js_dyn_index_get` dispatcher). The receiver is
// a *number* — so `[idx]` must return `undefined` (Node semantics:
// `(1.7e-314)[0]` is undefined), and Perry must NOT crash.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// Build a denormal-bits f64 the same way effect's fiber loop happens to.
// 0x8_0000_0000 raw bits ⇒ subnormal ~1.7e-314. Type as `any` so the
// codegen sees Any/Unknown and routes through `js_dyn_index_get`.
const buf = new ArrayBuffer(8);
const u = new BigUint64Array(buf);
u[0] = BigInt("0x800000000");
const f = new Float64Array(buf);
const denormal: any = f[0];

// 1. typeof is number for any f64 bit pattern, including denormals.
console.log("denorm-is-number:", typeof denormal === "number");

// 2. The hot path: `obj[idx]` where the receiver is the denormal number
//    (typed Any so this routes through `js_dyn_index_get`). Indexing a
//    number returns undefined in JS — Perry must match, NOT SIGBUS.
const idx: any = 0;
const result0 = denormal[idx];
const result1 = denormal[1 as any];
const result2 = denormal[7 as any];
console.log("idx0:", result0);
console.log("idx1:", result1);
console.log("idx7:", result2);

// 3. Many adjacent bit patterns that ALSO pass the heuristic's range
//    check (non-zero, < 2^48, > 0x10000, low-2 bits zero) — none should
//    crash, all should produce `undefined`.
for (const raw of [
  "0x10000",
  "0x20000",
  "0x100000000",
  "0x800000000",
  "0xFFFFFFFFFFF8",
]) {
  u[0] = BigInt(raw);
  const v: any = f[0];
  const r = v[0 as any];
  console.log("probe", raw, "→", r);
}

// 4. Final sentinel — we got here without crashing.
console.log("done");
