perf: bitwise operators on unproven operands no longer pay a software `fmod` or a 25-instruction ToInt32 per operand (#10511).

`&` `|` `^` `<<` `>>` `>>>` and `~` over a destructured, `number`-annotated or untyped value (noble's `let { A, B, … } = this`, jsbn's `v & 0x3ffffff`, bcryptjs's Feistel loop) had three costs stacked on every operator. #10418 had already replaced the unconditional `js_dynamic_bit*` call with a tag-guarded diamond; this change fixes what was left.

- **Runtime ToInt32 used `fmod`.** `dyn_to_int32`/`dyn_to_uint32` (`value/dynamic_arith.rs`) computed `v.trunc().rem_euclid(2^32)` for every operand, including operands that were already int32. That links a software `fmod` (a bit loop whose cost grows with |v| / 2^32, so jsbn's ~2^48 digit products were the worst case) plus a software `trunc`. It was 38 % of the issue's profile. Now, for |v| < 2^63 (every int32 and nearly every real operand), the value is truncated to i64 and its low 32 bits are kept: one `cvttsd2si`. Larger finite values are reduced from their exponent and mantissa bits in a `#[cold]` helper. NaN and ±Infinity map to 0. No floating-point remainder remains on any path. This is the cold arm of every guarded bitwise operator, and `~x` on an unproven operand calls it on every evaluation (#10512).
- **The guarded arm converted with a ~25-op tower.** The #10418 diamond tagged-tested each operand and then ran `toint32_wrap`, a branchless exponent/mantissa ToInt32. Its test is now a single `|v| < 2^63` compare (`emit_is_int64_exact_number` in `expr/binary.rs`). Every NaN-boxed tag is a NaN bit pattern, so the compare rejects them exactly as the tag test did. It also rejects NaN, ±Infinity and |v| >= 2^63, which are the only Numbers ToInt32 has to special-case. The numeric arm is therefore `fptosi` to i64 plus `trunc`, and everything else goes to the helper. Once a diamond exists, proven-Number operands are range-tested too. With no guard at all, `toint32_wrap` still covers every Number.
- **Int32 bitwise results were not known to be int32.** `&` `|` `^` `<<` `>>` compute a BigInt only when both operands are BigInts, since a mixed pair throws. So once either operand provably is not a BigInt, the result is an int32 Number whatever the other holds. `is_known_i32_range` now says so (`is_int32_bitwise_result` in `expr/i32_fast_path.rs`). Consumers such as rotr's outer `|`, `(B & C) ^ ((B ^ -1) & D)` and the `const t = … | 0` slot store then convert with `fptosi` instead of the tower. This is deliberately not an `int_range_expr` range: range consumers such as the integer typed-array store may lower an in-range expression natively, operands included, which an unproven operand must not be.

`fptosi double → i64` followed by `trunc` stays 64-bit wide under LLVM 22 on both x86-64 (`cvttsd2si %rax`) and AArch64 (`fcvtzs x`), so the pair is exact for |v| < 2^63.

Measured with the issue's `bench.ts`, N = 20M, `PERRY_NO_AUTO_OPTIMIZE=1`, x86-64 Linux, 4 vCPUs. Figures are medians of 3 runs in ms. All checksums are identical to Node's.

| variant | before | after | speed-up | helpers forced before → after (`PERRY_GUARDED_ARITH=0`) |
|---|---:|---:|---:|---:|
| destructure | 1,470 | 483 | 3.0× | 2,445 → 687 |
| annotated | 1,448 | 487 | 3.0× | 2,359 → 700 |
| or0 (control, still has `~B`) | 566 | 357 | 1.6× | 592 → 386 |
| prop (untyped property operand) | 724 | 538 | 1.3× | 1,257 → 613 |
| bigmask (`v & 0x3ffffff`, \|v\| ≈ 2^48) | 901 | 856 | 1.05× | 1,516 → 879 |
| or0X, bigarith (controls) | 31 / 885 | 31 / 916 | — | — |

Not closed by this change. `destructure` is still ~15× the `or0X` control. The destructured locals are boxed JSValue slots because nothing proves they stay Numbers, so every `D = C; C = B; …` shuffle pays a GC root store, a barrier check and a string-addref test, and each bitwise result that may be a BigInt is rooted. Removing that needs local type speculation (a loop-entry guard on the locals' types), which is independent of the operators. `~x` on an unproven operand is still a call per evaluation (#10512).

Validation:
- New `test-files/test_gap_10511_bitwise_unproven_operands.ts` covers the issue's shapes, every operator × 39 magnitudes around the ToInt32 boundaries (2^31, 2^32, 2^53, 2^63, 1e20, MAX_VALUE, NaN, ±Infinity) × 12 right-hand sides, compound assignment, non-Number operands through ToNumeric, BigInt pairs, mixed-BigInt throws and conversion order. It matches Node byte for byte, as do the existing bitwise/ToInt32 gap tests.
- New runtime unit tests compare ToInt32/ToUint32 with the old `rem_euclid` oracle over spec values from Node, every biased exponent, powers of two ±1.5, and 200k random bit patterns. They also cover every `js_dynamic_bit*` helper on large operands.
- New codegen unit tests check that the guarded arm is range-tested with no ToInt32 tower, and that a bitwise result with a Number operand converts without the tower. A `Number(a) | Number(b)` control shows the detector still fires. That test fails with the int32 fact disabled.
- `cargo test -p perry-codegen` passes.
