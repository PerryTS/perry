perf(codegen): keep bitwise NOT `~x` inside the native int32 chain (#10512). Over a Number, `~x` is `x ^ -1`, but Perry lowered it through a double: ToInt32 the operand, `xor`, then `sitofp` back out. `Expr::Unary` also had no arm in the i32 chain, so a single `~` pushed its whole enclosing `& ^ + | 0` expression onto the f64 path, and every enclosing operator ran the ~25-instruction ToInt32 tower again on the way back in. A sha256 `Chi` round, `(B & C) ^ (~B & D)`, ran 10–15× slower than the same loop spelled `B ^ -1`.

`~` now lowers the way `x ^ -1` does:

- `i32_chain_magnitude_bits` and its region-aware twin admit `Unary { BitNot }` exactly when they would admit `x ^ -1`, with the same 32-bit result bound. The #7232 2^53 exactness cap still applies to the operand, so `~(x * 1103515245)` stays on the f64 path.
- The structural i32 lowering emits `xor i32 %x, -1`.
- `lower_expr_value` gains a `BitNot` arm (`unary::lower_bitnot_value`). It returns a native `i32` whenever the operand has a Number-proven int32 form, so `~x` also stays native when it is not part of a larger chain. A BigInt or possibly-BigInt operand still reaches `js_dynamic_bitnot`.

Both native paths lower the operand through `lower_bitwise_operand_i32`, the binary bitwise operators' operand path. That path ToInt32-*wraps* a bare integer-valued local that has no i32 slot. The chain's own leaf conversion does not: it is a bare `fptosi`, which is poison for an integer local that has grown past int32. Reusing it made `~c & 0xffff` print `0` for a ~1.5e18 `c` during development. The new gap test pins that case. `x ^ -1` still has the problem on such locals. It predates this change and is not fixed here.

Measured on the issue's `bench.ts` (Linux x86-64, `PERRY_NO_AUTO_OPTIMIZE=1`, N = 20M, loop ms):

| variant | before | after |
|---|---:|---:|
| `sha_not` (`~B`) | 463–487 | 20.0–20.4 |
| `sha_xor` (control) | 18.7–21.6 | 19.3–20.9 |
| `not` (`~x`) | 229–256 | 1.2–1.5 |
| `not_xor` (control) | 1.1–1.5 | 1.1–1.5 |

With symbols kept, `sha_not`/`sha_xor` and `not`/`not_xor` now disassemble to identical instruction streams; they differ only in jump-target labels and RIP-relative constant addresses. Checksums are unchanged. Node 22 on the same host ran the `~` variants in 49 ms and 13 ms.

Tests:

- `bits_tests.rs` asserts `~x` is admitted exactly when `x ^ -1` is, including the rejected cases.
- `unary_bitnot_tests.rs` builds `((b & d) ^ (~b & d)) | 0` over int32 locals and asserts its IR is byte-identical to the `b ^ -1` spelling.
- All four new unit tests fail with the fix reverted.
- `test-files/test_gap_10512_bitnot_i32_chain.ts` covers the issue's loops, int32 boundaries, operands past int32 and past 2^63, NaN/±Infinity/fractions, typed-array operands, `x & ~mask`, BigInt `~`, coercions and `~s.indexOf(…)` truthiness.

The issue's two optional follow-ups are not in this change: an i64-range `fptosi` for non-chain operands, and a cheaper x86-64 ToInt32 than the shift/select tower. No version bump.
