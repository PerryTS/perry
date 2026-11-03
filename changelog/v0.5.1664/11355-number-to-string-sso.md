perf(runtime,codegen): `String(n)`, `` `${n}` `` and `n.toString()` return a short number's text as an inline SSO string instead of allocating a heap one (#10762).

`"" + n` has always gone through `js_string_concat_value_box`, which returns an
`f64` and packs a result of up to 5 bytes into `SHORT_STRING_TAG`. The other
three spellings called `js_string_coerce` / `js_template_string_coerce` /
`js_jsvalue_to_string_method`, which return `*mut StringHeader` and so had to
allocate a heap string for a three-byte result (or, for `0..256`, probe the
thread-local small-int cache). Codegen NaN-boxed the result right after the
call anyway.

- New NaN-box-returning twins — `js_string_coerce_box`,
  `js_template_string_coerce_box`, `js_jsvalue_to_string_method_box` and
  `js_number_to_string_box` — and the `String(x)`, `${x}` and universal
  `x.toString()` lowerings call them. A plain number formats straight into SSO
  bits when its text fits (every integer in `-9999..=99999`, `NaN`, short
  fractions such as `"0.5"` / `"123.5"`); a string argument comes back
  unchanged instead of being materialized; everything else forwards to the
  pointer-returning original, so no non-number arm changes.
- Integers that fit SSO are packed directly from the value, with no stack
  buffer and no `f64::fract()`. On the baseline x86-64 target `fract()` is a
  libm `trunc` call (about 15 instructions). The same range-test-plus-round-trip
  replaces `fract()` in `format_number_into`'s i32 arm, in
  `js_number_to_string`'s cache admission and in `js_string_concat_value_box`'s
  SSO arm.
- `"" + n` with an empty prefix now uses the same integer packer. That covers
  negative integers too, which `is_plain_f64` (sign bit read as a tag) used to
  send to `js_jsvalue_to_string` and a heap string.
- `proven_heap_string_operand` no longer lists `String(x)` / `${x}`, because
  they can now be SSO. `str_operand_handle_tag_dispatched` gives them the
  heap/SSO two-arm dispatch that canonical-`Str` locals already use.
- `scripts/gc_root_dominance_check.py`: the three coercion twins are in
  `POLL_CAPABLE_RUNTIME` (they forward object arguments to user
  `toString`/`valueOf`), and `string_coerce_box|template_string_coerce_box` are
  in `ALLOC_RE`, so the checker keeps tracking these results the way it tracked
  `js_string_coerce`'s.

Measured with valgrind instruction counts (Ir per loop iteration, fitted over
N = 200k → 2M, fixtures compiled with `--march x86-64-v3`; each iteration
converts `n` and reads `.length`; the no-conversion control loop is 46.0 on
both arms):

| spelling | base | fix |
|---|---|---|
| `String(k % 100)` | 156.0 | 145.6 |
| `` `${k % 100}` `` | 171.0 | 146.6 |
| `"" + (k % 100)` | 239.1 | 161.6 |
| `(k % 100).toString()` | 153.0 | 151.6 |
| `String(k % 1000)` | 428.3 | 158.5 |
| `` `${k % 1000}` `` | 443.3 | 159.5 |
| `"" + (k % 1000)` | 261.4 | 174.5 |
| `(k % 1000).toString()` | 425.3 | 164.5 |
| `String(k % 1e6)` | 563.3 | 483.3 |
| `String((k % 1000) + 0.5)` | 1235.4 | 970.0 |

The four spellings used to span 153–443 across the two value ranges and now
span 146–175. Six-digit integers and long fractions still allocate: their text
does not fit SSO. Closing the remaining gap to bun (~30 Ir) needs the
conversion inlined at the call site, which is not part of this change.

Coverage: `string/number_to_string_box_tests.rs` checks that every twin prints
byte-for-byte what its pointer original prints (edge values plus 50k random
doubles, every integer in `-9999..=99999`, the 5/6-byte SSO boundary, `-0`,
non-finite values, strings passed through unchanged, other primitives). It also
checks that short results are canonical SSO bits.
`test-files/test_gap_10762_number_to_string_sso.ts` runs the SSO results through
`.length`, equality with heap strings, object/Map/Set keys, string methods,
JSON, joins, `switch` and `in`, and checks that Symbol, nullish and object
arguments behave as before.
