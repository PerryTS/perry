perf(codegen): user-visible ToInt32 takes a guarded hardware conversion
instead of an unconditional ~25-op tower (#10897).

`x | 0`, `~x`, the operands of `&` `|` `^` `<<` `>>` `>>>`, integer
typed-array stores and the entry conversion into a canonical i32 slot all
lowered through `LlBlock::toint32_wrap`: the exact, branchless
exponent/mantissa ToInt32 (needed because LLVM `fptosi` is poison outside its
destination range, so a bare conversion printed garbage for `(1e20) | 0`).
It ran in full on every evaluation, so on a double accumulator `| 0` cost more
than the property read beside it — +43 instructions/iteration for
`h = (h + o.a) | 0` over `h = h + o.a`, while node gets faster with the `| 0`.
Every `mr*`/`ops4` regression fixture and the four #10761 parity operations
spell their accumulator that way, so the property-read guard has been
measuring this alongside the read.

Every Number with `|v| < 2^63` is exact through `fptosi double -> i64`, and the
low 32 bits of that truncation ARE ToInt32. The new `FnCtx::toint32_wrap`
(`expr/toint32.rs`) tests `fabs(v) < 2^63` (ordered, so NaN fails it), takes
`fptosi -> i64` + `trunc` on the predicted arm and runs the unchanged tower —
renamed `LlBlock::toint32_wrap_branchless` — only on the cold arm for NaN,
±Infinity and `|v| >= 2^63`. That is the V8/JSC shape: `cvttsd2si` plus a
rarely-taken fixup. FEAT_JSCVT targets (apple-arm64) keep the single
`fjcvtzs`. A double the current block just widened from an i32
(`sitofp`/`uitofp i32`) converts back to that i32 with no instructions, which
removes the second conversion when a `| 0` result enters a canonical i32 slot
(LLVM does not fold the magnitude guard on a `sitofp` result).

Measured marginal instructions/iteration (valgrind cachegrind, N=500k→5M,
`PERRY_TARGET_CPU=haswell`, x86-64), output identical to node on every row:

| fixture | loop body | before | after |
|---|---|---:|---:|
| `fpxnum` | `h = h + o.a;` | 25 | 25 |
| `pxnum` | `h = (h + o.a) \| 0;` | 68 | 14 |
| `bitmix` | `h = (h ^ p.a) \| 0; x = (x + (p.b >>> 1) + (~p.a & 255)) \| 0;` | 306 | 164 |

`| 0` now makes the accumulator loop cheaper than the plain `+`, as it does in
node.

Also fixed while pinning the boundary: the `js_uint8array_set` /
`js_buffer_set` fallback stores converted the value with a bare
`fptosi double -> i32`, so on x86 `u8[i] = 4294967295` (or `-2147483649`)
stored 0 instead of 255. Both now use the same exact conversion.

Validation: `expr/toint32_tests.rs` pins the diamond on emitted IR for every
converting operator, the tower confined to the exact arm, the `fjcvtzs` path,
and the single conversion for a canonical-i32 slot store (each assertion was
sabotage-checked against the old lowering).
`test-files/test_gap_toint32_fast_path_boundary.ts` drives runtime values
across the `2^63` boundary (largest doubles below it, ±2^63, 2^64 + 4096,
1e20, ±MAX_VALUE, denormals, ±0, ±Infinity, NaN) through every operator, a
class-field accumulator, a hash mixer whose products land on both arms, and
Int32/Uint32/Int16/Uint8Array and Buffer stores — byte-identical to node.

Not addressed: the native i32 lowering's generic fallback
(`expr/i32_fast_path.rs`, `lower_expr_native(.., I32)`) still converts an
arbitrary double with a bare `fptosi -> i32`; the native Uint8Array store fast
path reaches it for a constant-index store of an out-of-int32 double.
