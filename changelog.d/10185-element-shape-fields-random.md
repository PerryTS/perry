Serve the JSON access benchmark's `fields` and `random` shapes from the
element-shape versioned loop clone (#7480 / #5093 / #10171). Stacked on
#10171, which made the clone fire for `JSON.parse`'d record arrays at all.

#10171 took the benchmark's `repeat` and `sequential` modes to parity. The two
remaining modes got nothing, for three separate reasons — and because a loop is
admitted as a whole, any one of them cost the entire clone:

```ts
// random: a loop-carried index
cursor = (cursor * 17 + 7) % length;
const index = cursor;
sum += rows[index].id;

// fields: three accumulator statements, one string read, one boolean ternary
const index = i % length;
sum += rows[index].id;
sum += rows[index].name.length;
sum += rows[index].active ? 1 : 0;
```

* **A loop-carried index** (`stmt/element_shape_carried.rs`). The recurrence is
  folded to one affine pair `(a, b)` and evaluated as `srem i64` — the generic
  `%` is an `frem`, which on aarch64 is a libm call, and a call inside this
  clone does not slow it down, it DELETES it (#7690). Three obligations, all
  discharged once in the preheader: the modulus materializes as an i32 in
  `1..=i32::MAX` (so `m <= length` puts every index in bounds with no per-read
  test), the carried binding's ENTRY value materializes as a non-negative
  integral i32, and `a`/`b` are required non-negative so the dividend cannot go
  negative — JS `%` returns a *negative* remainder for a negative dividend,
  which is an out-of-bounds subscript rather than a slow path.

  The matcher also tracks the largest magnitude any sub-expression of the
  recurrence can reach and declines above 2^53. "Fits i64" is necessary and NOT
  sufficient: JavaScript evaluates the recurrence in doubles, and
  `cursor * 1e9 + 1` fits i64 comfortably while losing its low bits in the f64
  the program actually runs. The bound is tracked per NODE rather than taken
  from the folded pair, because folding cancels: `c * 1000 - c * 999` is
  `1 * c`, but JavaScript still evaluates both thousand-fold products.

* **Where the carried write-back goes is a correctness question, not a
  scheduling one.** The residual side exit resumes the CURRENT iteration in the
  slow clone, which re-runs the whole body — the recurrence included. So the
  commit to the real binding is the LAST statement of the iteration: a
  mid-iteration exit then leaves the binding holding that iteration's entry
  value and the slow clone advances it exactly once. A write-back at the update
  site double-applies it, and every later index is silently a different — still
  in-bounds, still a valid record — one.

* **K accumulator statements**, folded into one for the fast clone
  (`acc = a; acc = acc + b` ≡ `acc = (a) + b`: the same operations on the same
  operands in the same order, so the float result is bit-identical). Same
  reason as above — the whole iteration must commit once, past every exit it
  can take, or a tag test that fails on the third read leaves the first two
  already applied when the slow clone re-runs it. Statements 2..K must read the
  accumulator as the LEFT operand and must not read it in the addend, or the
  substitution would change which value the addend sees.

* **`arr[i].prop.length` and `arr[i].prop ? A : B`**
  (`expr/element_shape_reads.rs`). The preheader proves which inline slot holds
  a property; it proves nothing about what is in it. So each read tag-tests the
  loaded word and side-exits when the answer is not the representation it is
  about to assume: both string forms for `.length` (heap `utf16_len`, and the
  SSO length byte — the same decode the runtime's own `.length` arms use), and
  the two boolean singletons by exact NaN-box bit pattern for the ternary.
  JS truthiness of `0` / `""` / `null` / an object is a runtime question the
  clone does not guess, and a non-constant ternary arm is not admitted at all.

* **One residual check per iteration, not one per read.** Three reads of
  `rows[index]` were three element loads and three header/ShapeId checks. The
  body's leading virtual binding now emits the deref and the residual once and
  parks the masked handle in an entry alloca. That is also what makes the
  multi-read side exit correct: every residual exit now precedes every store.

* **The property denylist is arm-specific now.** The class-keyed arm keeps the
  full list — its read bakes in a compile-time packed slot index while the
  surrounding lowering may route the name elsewhere. The shape-keyed arm bakes
  in nothing: it asks the runtime for that exact key's inline slot in that exact
  ordinary ShapeId and declines on `-1`, and the residual pins
  `obj_type == GC_TYPE_OBJECT` with no per-object descriptors — so every
  receiver whose builtin branch could answer a name differently (a function's
  `name`, an array's `length`, a Map's `size`) is already excluded, and for a
  plain record an own data property SHADOWS the prototype name it collides
  with. Only `__proto__` stays denied, and not because of JavaScript (node
  gives `JSON.parse('{"__proto__":1}')` an own data property and reads `1` back
  from it): Perry's generic property path may special-case the name ahead of
  own-property lookup, and the clone must agree with the path it is a clone of
  wherever the two differ. The full list would otherwise have cost the
  benchmark its `fields` mode outright, for a field literally called `name`.

**Measured** (ns/iter, 1M iterations, 5 interleaved rounds, best of 5,
`PERRY_NO_AUTO_OPTIMIZE=1`, on a loaded shared host; `new` and `base` are both
built from this worktree, `base` at #10171's head `2b77e7d4fe`):

| cell | base (#10171) | new | node 26.5.1 | bun 1.3.14 | new / best |
|---|---:|---:|---:|---:|---:|
| 16k `repeat` | 4.21 | 4.17 | 3.56 | 5.23 | 1.171 |
| 16k `sequential` | 4.17 | 4.30 | 4.57 | 6.25 | 0.941 |
| 16k `random` | 15.48 | **4.97** | 7.16 | 9.96 | **0.694** |
| 16k `fields` | 25.59 | **6.05** | 5.88 | 9.77 | **1.029** |
| 1m `repeat` | 4.15 | 4.17 | 3.19 | 5.05 | 1.307 |
| 1m `sequential` | 4.26 | 4.20 | 9.42 | 7.96 | 0.528 |
| 1m `random` | 16.00 | **5.32** | 11.71 | 11.17 | **0.476** |
| 1m `fields` | 28.14 | **6.17** | 12.70 | 14.91 | **0.486** |
| 20m `repeat` | 4.12 | 4.11 | 3.50 | 5.24 | 1.174 |
| 20m `sequential` | 4.20 | 4.19 | 6.98 | 8.51 | 0.600 |
| 20m `random` | 27.53 | **5.63** | 9.31 | 10.56 | **0.605** |
| 20m `fields` | 27.05 | **6.06** | 10.98 | 14.81 | **0.552** |

The two targeted shapes move 3.0x-4.9x and land below the better of node and
bun on five of their six cells. The sixth, 16k `fields`, is the one that does
not reach parity: 6.05 against node's 5.88 (1.029x; 6.08 vs 5.83 on a
separate 12-round re-measure of just that pair).
That is the smallest fixture, where the whole array is in L1 and the guard
overhead is the only thing left — node's own `fields` is just 1.29x its
`sequential` there, because its inline caches have type feedback proving `name`
is a string and `active` a boolean, while this clone tag-tests both on every
read. Paying that test is what lets it side-exit instead of deoptimize, so it
is a floor of the design rather than a missing peephole. Reported rather than
rounded off.

`repeat` and `sequential`, which #10171 already served, are unchanged within
noise in both wall clock and instruction count — `sequential`'s single read has
nothing to share, and `repeat`'s constant index has no leading binding to hang
the prologue off.

Instructions retired per iteration on the 16k fixture (2M iterations minus a
0-iteration run, `/usr/bin/time -l`):

| shape | base | new |
|---|---:|---:|
| random | 207.6 | 29.1 |
| fields | 466.4 | 47.0 |
| sequential | 25.0 | 25.6 |
| repeat | 16.0 | 16.1 |

`random` lands one recurrence above `sequential`'s 25, and `fields` two extra
guarded reads plus a string decode and a select above it — which is what the
shared residual buys, and the shape the numbers have to have if the clone is
really being entered rather than merely emitted (#10171's own lesson: a clone
can be `cond_br`-entered and never executed while every IR-census assertion
passes, and only the wall clock notices).

Validation: `test-files/test_gap_json_record_loop_clone.ts` gains 40 cases —
the recurrence with and without its alias, the carried value read after the
loop, negative and fractional entry values, a zero and an over-long modulus, a
multiplier past the exact-double range, a mid-loop side exit whose sum AND
final cursor both observe the write-back protocol, SSO and heap `name`s, a
non-string and a null `name`, five non-boolean `active` values, a two-statement
fold whose second read side-exits, and records with own `name`/`length`/`size`
properties — all byte-identical to `node --experimental-strip-types` 26.5.1.
70 codegen IR-census tests (`element_shape_fields_random_tests.rs` and its
siblings), every positive paired with a sabotage case asserting the clone is
ABSENT. Seeded GC stress on the gap binary
(`PERRY_GC_SCHEDULE_SEED=1..4 PERRY_GC_SCHEDULE_RATE=0.2
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=32`): output
identical across all four seeds and to node, with 3–11 copying minors and
~14.4k moved objects per run, so the instrument's subject was live.
