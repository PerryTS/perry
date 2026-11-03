Keep the element-shape loop clone's accumulator in the double domain and its
counter in the i32 domain (#7480 → #10171 → #10185). The JSON access screen's
four missing cells (`repeat` ×3, 16k `fields`) now beat Node and Bun at the
official settings, and every clone form got cheaper.

**The defect.** The fast clone was already what executed, but the `repeat` loop
(`sum += rows[7].id`) took 16 instructions at ~13 CPU cycles per iteration: two
loop-carried chains crossed the integer/float register boundary every
iteration.

* An `any`-typed `sum` lives in a precise GC root slot, and every root reload
  passes through the RS4GC launder (`function/precise_roots.rs`,
  `ROOT_RELOAD_LAUNDER`) that LLVM cannot see through, so mem2reg promoted the
  slot as NaN-box `i64` bits: `x25 → fmov → fadd → fmov → x25`.
* A counter the clone never indexes with (the constant-index and carried forms)
  had no i32 slot, so it stayed a double (`fadd d8, #1.0`) compared against
  `count` — itself a root slot, reloaded through the launder and moved to an FP
  register every iteration although the preheader had already materialized it
  as an i32.

**The change** (`stmt/element_shape_native.rs`). For the fast clone's lowering
the accumulator is redirected into a promotable `alloca double` (the
`numeric_accumulator_f64_slots` redirect the packed clones already use), seeded
with the value the deref block tag-tested as a Number; the counter gets an i32
slot (its Let-site parallel slot, or a clone-private one seeded with the literal
start) that the `Update` lowering advances alone
(`deferred_integer_update_accumulators`), so the precomputed i32 trip count
turns the condition into `icmp slt i32`. Residual checks branch to a
`element_shape.loop.side_exit` trampoline that publishes both scalars to their
real slots and then enters the slow clone; the fall-through exit publishes them
in `element_shape.loop.fast.write_back`. A canonical-i32 counter (the indexing
forms) has nothing to defer and is left alone.

Soundness is unchanged by construction: the redirected store sits exactly where
the root-slot store did, which #10185's fold / carried-commit ordering already
put after every side exit of the iteration, so the trampoline publishes the
iteration's entry accumulator and its own index — the state the slow clone
re-runs it from. The carried binding keeps its own end-of-iteration commit and
is never published by the trampoline. The seed, trampoline and write-back are
plain loads/stores plus one `sitofp`, created inside the call-free scan's block
range. The accumulator was already consumed as a raw double inside this clone
(`numeric_accumulator`), so the IEEE `fadd`s are the same operations on the same
operands in the same order (`-0`, NaN, overflow to Infinity bit-identical). The
i32 counter only ever runs against a trip count that is a literal, `arr.length`
or `materialize_loop_i32`'s integral `0..=i32::MAX`; fractional, NaN, negative
and out-of-range bounds still route to the slow clone.

**The new `repeat` loop** (arm64, `run$spec_b_b`, whole body):

```
a90 cbz  w9, side_exit            ; residual check (header loads hoisted)
a94 ldr  d2, [x10, w0, sxtw #3]   ; field load
a98 fmov x12, d2
a9c cmp  x12, x11                 ; Number tag test
aa0 b.gt side_exit
aa4 fadd d1, d1, d2               ; sum stays in d1
aa8 add  w8, w8, #1               ; i is an i32
aac cmp  w19, w8                  ; against the materialized i32 count
ab0 b.ne a90
```

**Per iteration** (`/usr/bin/time -l`, 50M iterations, 16k fixture):

| mode | instructions main → PR | cycles main → PR |
|---|---:|---:|
| repeat | 16.0 → 9.0 | 13.0 → 3.0 |
| sequential | 25.0 → 22.0 | 13.0 → 3.4 |
| random | 29.0 → 27.0 | 15.6 → 15.6 |
| fields | 51.0 → 47.0 | 19.0 → 9.0 |

**Access screen, official settings** (1M iterations, warmup 0; bench mini, best
of 9 interleaved rounds, ns/iteration): `repeat` 4.06 → 0.94 (Node 2.89–3.07,
Bun 4.47–4.60); `sequential` 4.06–4.16 → 1.07/1.63/3.31; `random` 4.86–5.45 →
4.80–5.39; `fields` 5.94–6.03 → 2.81/2.82/3.39 (16k Node 5.57). All 12 cells at
or better than the better of Node and Bun (worst ratio 0.70, 16k `random`).

**Warmed** (50M iterations, 1M warmup, best of 5): 3 of 12 cells win (16k
`sequential`, 1m and 20m `fields`); 9 still miss, each for a structural reason
this change does not address:

* `repeat` 0.94 vs 0.38/0.52/0.31 — one IEEE add per iteration is 3 cycles of
  latency on this core; the JITs' 1.0–1.6 cycles mean an int32 speculated
  accumulator, which JS double semantics for `sum` do not license here.
* `fields` 16k 2.81 vs Node 2.77 — three serial IEEE adds = 9 cycles (measured
  9.02), the double-domain floor for the source-order fold.
* `random` 4.86/5.09/5.39 vs 4.63/4.47/4.64 — the loop-carried chain is the
  recurrence's division (15.6 cycles, unchanged by this change); the i64 `srem`
  #10185 needs for exactness is ~5 % slower than an int32 division (same-host C
  microbenchmark: 4.69 vs 4.46 ns), which is the whole 16k gap. The larger 1m/20m
  gap is not in the loop code: Perry's 27 instructions are identical across
  sizes while its cycles rise 15.6 → 16.3 → 17.3, and Bun gets FASTER than its
  own 16k number — a record-memory locality difference, not investigated here.
* `sequential` 1m/20m 1.65/3.38 vs 1.32/2.02 — identical 22 instructions but
  3.4 → 5.2 → 10.9 cycles from 16k to 20m: memory-bound record access, the same
  locality difference.

**JSON matrix** (50 rows, 5 interleaved rounds, best-of, PR vs main): 48 rows
within ±2 % CPU (the one element-shape consumer, `scan`, 0.5–1.1 % faster), no
row's peak RSS higher. Two runtime-only rows moved further, one each way:
`long_string_1m:stringify` −4.4 % and `escaped_1m:stringify` +10.3 % (160.8 →
177.3 ms). Neither runs a clone; the latter's hot function
`json::stringify_flat::emit_piece` is identical runtime code shifted 128 bytes
by the smaller worker module, and appending unused functions to main's own
worker moves the same row to 149.0–149.4 ms — layout, not this change.

Tests: five IR-census tests in `stmt/element_shape_native_tests.rs` (each fails
with the redirects disabled); the #10185 carried-commit and side-exit helpers
now count the trampoline spelling of an exit. `test_gap_json_record_loop_clone.ts`
gains `-0`/Infinity/NaN accumulation, first-iteration and mid-loop side exits
with a non-zero accumulator, fractional/NaN/negative/string trip counts on the
`repeat` form and a late `random` side exit; byte-identical to Node 26.5.1.
