**Reassigned numeric accumulators reach the inline arithmetic fast path
(#8105).** Every numeric proof `type_analysis::is_numeric_expr` had for a bare
`LocalGet` was either an integer-range fact or `stable_local_type_proof`, and
that one answers `None` the moment the local is written a second time. A plain
fractional accumulator therefore had no numeric proof at all, so
`expr/binary.rs`'s "both operands are statically primitive" test failed and
every `*`/`/`/`%`/`-`/`**` on it bailed to the BigInt-aware dynamic helper.
`benchmarks/suite/15_mandelbrot.ts`'s `let x = 0.0; … x = x * x - y * y + cx;`
inner loop emitted six `call double @js_dynamic_mul` per iteration; `+` and `-`
stayed inline because a `Binary { op: Mul, .. }` operand IS numeric by the
recursive rule, which is why the symptom read as "only the multiplies escape".
Wrapping the loop in a `number`-typed function did not help — even the fully
specialised `$spec_b_b_i32` clone kept the calls, because the gap is about the
body local, not the signature.

New `collectors/number_by_construction.rs` exposes the existing #7770
number-by-construction locals fixpoint (`collectors/ptr_shape_numeric.rs`)
outside the `Ptr<Shape>` pass, and `is_numeric_expr`'s `LocalGet` arm consults
it. That fixpoint is already trusted for a strictly harder claim — it licenses
a bare `load double` on a proven numeric FIELD with no coercion and no value
check — and a JS Number's Perry representation IS its raw double, so the proof
is exactly "the f64 in this slot is a canonical double". It is fail-closed:
candidates are the ids with a `Stmt::Let` in the scanned body, so parameters
(unconstrained incoming value, readable before the first assignment) and
locals captured from an enclosing scope (writes outside the walk) are never
admitted; closure-boxed locals and module globals are excluded outright; and a
`let x;` with no initialiser is `undefined`, which drops the local. Declared
types are never evidence (#7773). Gated by `PERRY_NUMBER_BY_CONSTRUCTION`
(default on; `=0`/`off`/`false` empties the fact), keyed into both the
build-level probe and the object cache.

Measured on macOS arm64 with one compiler under the flag on/off, so both arms
link the same `libperry_{runtime,stdlib}.a` bytes; `PERRY_RUNTIME_DIR` pinned,
`PERRY_NO_AUTO_OPTIMIZE=1`, `/usr/bin/time -l`, medians of 5, outputs
identical:

| benchmark | instructions on | instructions off | delta |
|---|---:|---:|---:|
| `15_mandelbrot` | 211,541,019 | 1,709,580,209 | **-87.6%** |
| `06_math_intensive` | 375,839,338 | 1,976,950,801 | **-81.0%** |
| `13_factorial` | 1,026,942,694 | 4,228,008,602 | **-75.7%** |

Peak memory footprint moves with it or stays flat (`15_mandelbrot`: 2,163,096
B on vs 2,228,656 B off, -2.9%); nothing regresses. The other 27 programs in
`benchmarks/suite` are within ±0.5% with identical output. `15_mandelbrot`
drops from 214 instructions per innermost iteration to 26, which is the first
time #7128's published "one basic block of ~13 instructions" — and the ±2
instruction `collectors/repsel_benefit.rs` cost model built on it — describes a
loop that exists.

Three tests, deliberately a positive plus two sabotage arms: the positive one
alone would also pass against an analysis that admits every reassigned local,
which is the wrong-code shape #7773 shipped. A local seeded from an `Any`
parameter, and a local with a single string write, must both keep #5970's
`js_dynamic_mul` routing.
