perf(codegen): number-to-string builds a small integer's text at the call site, and its result keeps its string proof (#10762).

- `String(n)`, `` `${n}` ``, `n.toString()` and `"" + n` on an operand the type
  analysis proves numeric now compute the SSO bits of an integer in
  `-9999..=99999` inline, with no call. The digits come from a fixed-point
  split (`abs * ceil(2^32 / 10^4)`, then `* 10` per digit, exact for every
  `abs < 100000`), about 30 branch-free instructions. Every other value
  (fractions, `NaN`, the infinities, larger integers, and a declared `number`
  that holds something else at run time) takes the original runtime call on
  a cold arm, so the text is unchanged, and the bits match
  `small_integer_sso_bits` exactly. An operand without a numeric proof keeps
  the plain call, so the code is emitted only where it is expected to fire.
- `const s = String(n)`, `` `${n}` `` and `"" + n` (a `+` with a string-literal
  operand) record a runtime-derived `String` proof for `s`. Before, the local
  lost the proof its initializer carries, and `s.charCodeAt(i)`, `s.length`
  and the other string lowerings fell to the generic method site, although
  `String(n).charCodeAt(i)` written inline took the fast path. A declared
  `string` is still not a proof (#7837).
- The inline `charCodeAt` reads an all-ASCII SSO receiver's byte straight out
  of the value. Before, an SSO receiver, which is what every short number's text
  now is, went to the slow arm, which materialized a heap copy through the
  intern table (about 175 instructions) to read one byte.

Measured (`callgrind`, instructions per iteration fitted over 20k→120k, x86-64,
`PERRY_NO_AUTO_OPTIMIZE=1`, `k = i % 1000`, the text consumed by
`charCodeAt(0) + length`): `String(n)` 449 → 169, `` `${n}` `` 462 → 169,
`String(-k)` 644 → 199, a fraction 1,251 → 987, and a 7-digit integer (a heap
string) 623 → 560 over 1M→3M. Consumed only by `.length`: `String(n)`
163 → 131, `"" + n` 186 → 133, `` `${n}` `` 172 → 131, `n.toString()`
160 → 133. A bare loop is 23.

Tests: `number_to_string_inline_tests` (each spelling emits the inline arm; an
`any` operand does not; `s.charCodeAt` on each coerced local reaches the SSO
arm), sabotage-checked, and `test_gap_10762_inline_number_to_string` (every
integer in the inline range against a reference, the range edges, lying
annotations, SSO `charCodeAt` including non-ASCII and bad indexes;
byte-identical to Node). No version bump.
