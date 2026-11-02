**A dynamically dispatched `x["toString"]()` on a number now produces
`NumberToString`, not Rust's `f64` Display** (#9713). It printed `inf` for
`Infinity` and the full decimal expansion past the exponential thresholds, so
the same value stringified four static ways and once dynamically disagreed
inside one program:

```ts
const a = 2.2e-308;
a.toString();                              // 2.2e-308   (all four static forms)
((x: any, m: string) => x[m]())(a, "toString");  // 0.000…00022 — ~308 digits
```

Three arms of the native-method tower — the plain-number and boxed-`Number`
`toString` in `dispatch_common`, and the boxed-`Number`
`toString`/`toLocaleString` in `dispatch_primitive` — formatted with

```rust
if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT { (n as i64).to_string() } else { n.to_string() }
```

`f64::to_string()` is Rust's shortest-round-trip Display, which never switches
to scientific notation and renders the infinities as `inf`. It is the exact
mistake `js_format_f64`'s doc comment already warns about — #3987 replaced the
same `format!("{}", n)` in the string-concat fast paths and these three arms
were not part of that sweep. They now call `js_number_to_string`, which carries
the spec's `|n| >= 1e21 || |n| < 1e-6` switch, the `Infinity` / `NaN` / `-0`
spellings, and its own (safer) integer fast path — `js_format_f64` cuts over to
the shortest-round-trip formatter at 1e15 rather than 2^53, so it also avoids
the `2**58` → `…744` vs `…740` divergence the local fast path could reach.

Measured against node 26.5.1, previously wrong and now correct: `1e21`,
`1e-7`, `-2.5e-9`, `2.2e-308`, `Number.MAX_VALUE`, `Number.MIN_VALUE`,
`Number.EPSILON`, `±Infinity`, and every one of those again through
`new Number(x).toString()`.

One neighbouring defect in the same arms rides along: a boxed receiver dropped
an explicit radix entirely, so `new Number(255).toString(16)` answered `"255"`
instead of `"ff"`. Both boxed arms now route an explicit radix through
`js_jsvalue_to_string_radix` the way the unboxed arm already did (which also
means an out-of-range radix throws `RangeError` there, as the spec requires).
`toLocaleString` keeps ignoring its argument — that one is a locale, not a
radix.

`test-files/test_gap_9713_dynamic_number_tostring.ts` pins 18 values across the
thresholds in all seven renderings plus the radix and `toFixed` /
`toPrecision` / `toExponential` forms. Unpatched it differs from node on 12
lines; patched it is byte-identical.

Not fixed here, and filed separately: `toString(radix)` above 2^53 for a
non-power-of-two radix still emits exact digits rather than V8's shortest
round-trip (#9725) — that one reproduces from a plain static call and is a
different formatter.
