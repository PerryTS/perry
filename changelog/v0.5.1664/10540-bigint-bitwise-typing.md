fix(codegen): BigInt `&` `|` `^` `<<` `>>` no longer typed as int32 (#10418).

These operators compute a BigInt when both operands are BigInts, but the compiler assumed every bitwise result is an int32 Number. For BigInt operands it could not prove statically (untyped params, `BigInt(...)` initializers, property/element reads, arithmetic results):
- `const x = a & b` read back as `0`.
- `Number(a & b)` returned the BigInt unchanged.
- A `bigint`-typed result reached a call as `fptosi` of its box.
- `(_2n << k) * _2n` threw "Cannot mix BigInt and other types".

This broke @noble/hashes' `fromBig` (`Number((n >> _32n) & U32_MASK64) | 0`): sha384/sha512/blake2b/blake2s/argon2 gave wrong digests and sha3/keccak threw at module load. Every @noble/curves-based package inherited the failure.

Root cause, three layers:
- HIR typing (`lower_types.rs`, `analysis/value_types.rs`) typed the operators `Number` over unknown operands.
- The integer-local disqualification judge (`collectors/integer_locals.rs`) and `collectors/int_valued_ta_locals.rs` accepted every bitwise write as int-producing, so the binding took an int32 slot that `ToInt32`'d the BigInt result.
- `Number()` elision (`expr/bigint_set.rs`) treated any bitwise operand as already a Number.

Fix:
- A bitwise result is a Number only when an operand provably is not a BigInt (`Type::is_non_bigint_primitive`). The not-BigInt fixpoint is exposed as `NotBigIntFacts` and computed ahead of the integer-local proofs.
- Bitwise ops whose operands are unproven now take the existing guarded numeric diamond (`lower_guarded_numeric_arith`: tag test, inline `ToInt32 <op> ToInt32` with a masked shift count, BigInt-aware helper on the cold arm). The Number case stays inline, and proven-Number operands keep the unguarded path.

Validation:
- `test-files/test_gap_10418_bigint_bitwise_typing.ts` covers operator × 16 operand shapes × 13 consumers, controls for `+ - * ** % ~`, `Number(n & M)`, and a noble `fromBig`/`split`/`toBig` round trip. Baseline: 97 lines differ from Node. Now: identical.
- New perry-hir and perry-codegen unit tests.
- @noble/hashes 2.2.0 sha256/sha512/sha3_256/keccak_256/blake2b/blake2s/blake3/sha384/hmac/argon2 digests match Node.
- Gap suite: no new failures.
- Instructions (no auto-optimize): SHA-256 compress −1.2 %, untyped integer-hash helpers −56 %, `bench_bitwise` unchanged.
