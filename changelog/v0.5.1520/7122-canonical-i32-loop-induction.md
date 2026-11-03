A bare loop counter never took canonical unboxed storage. Not in a function
body, not anywhere: `canonical_safe_local` in `stmt/let_stmt.rs` required the
local to be used as an **array index** or to sit in
`strictly_i32_bounded_locals`, and a counter is neither — `i++` disqualifies a
local from the latter outright (#6072), and nothing about `i` in

```ts
for (let i = 0; i < 1000000; i++) { sum = sum + 1; }
```

involves an array. Add one `a[i]` read and it promoted immediately. The
promotion turned on the presence of an array, not on any property of `i`.

## The proof that admits it

`collectors/loop_bounded_i32.rs` proves a **closed interval** the local can
never leave, from the guard that dominates every write to it:

* one declaration, initialiser an i32-range integer literal `I`;
* every write anywhere in the function is a step (`v++`, `v = v + k`, and the
  decrement mirror) with `k` a non-negative integer constant;
* every step sits directly in the body or update of a loop whose condition has
  a top-level `&&`-spine conjunct `v < B` / `v <= B` / `v > B` / `v >= B`, `B` an
  i32-range constant — a literal, a `const` local, or a module-level `const`
  from `compile_time_constants`;
* no intervening loop and no intervening closure between the step and that
  guard, so each step site runs at most once per iteration;
* the step direction agrees with the guard.

With `S` the sum of the steps at that level, an increment counter is confined to
`[I, B - 1 + S]` (`[I, B + S]` for `<=`), a decrement counter to the mirror, and
the local is admitted only when **both endpoints fit i32**.

This is a range argument, not a compatibility bound. The existing
`integer_locals ∩ index_used_locals` term is sound only because the pre-phase
shadow model already read the i32 slot for that exact set (the range-soundness
audit in `expr/slot_rep.rs` says so); this one adds no overflow surface, because
there is no reachable state in which the value leaves i32. The same argument is
already trusted one layer down — `stmt/loops.rs` allocates a *parallel* i32
shadow for a constant-bounded counter on exactly this reasoning. What is new is
lifting it to a Let-site fact, so the counter's i32 slot becomes its **only**
storage instead of a shadow kept in sync with a boxed double.

Consumed only by the canonical-i32 gate, never by the parallel-shadow
`needs_i32_slot` gate, so `PERRY_CANONICAL_I32_LOCALS=0` still reproduces the
pre-phase model bit-for-bit — the containment `int_valued_ta_locals` already
uses. No new env knob.

## The half that stays denied, and why

A bare **accumulator** is not admitted and must not be.
`benchmarks/suite/13_factorial.ts` — one of the three workloads #7110 names —
computes `sum = sum + (i % 1000)` over 1e8 iterations. That reaches
**49,950,000,000**, twenty-three times `INT32_MAX`. Node prints it exactly; an
i32 slot would print a wrapped negative. "Every write is `sum = sum + <integer>`"
is not an i32 proof, and a rule that treated it as one would be a silent wrong
answer rather than a missed optimization. Bounding an accumulator needs the
loop's trip count multiplied by a magnitude bound on the step expression —
strictly more analysis, and filed as #7123.

The `not_index_used_or_bounded` denial reason now says this, so the report
distinguishes "not implemented yet" from "must not be promoted".

## Evidence

`--opt-report=json --no-link`, macOS arm64, oracle Node 26.5.1.

```ts
function run(): number { let sum = 0; for (let i = 0; i < 1000000; i++) { sum = sum + 1; } return sum; }
```

| | `i` | `sum` |
|---|---|---|
| before | denied `not_index_used_or_bounded` | denied `not_index_used_or_bounded` |
| after | **selected `I32`** | denied `not_index_used_or_bounded` |

Emitted IR for that function, before → after: the counter's `alloca double`
becomes `alloca i32`, the condition's `load double` becomes `load i32`, and the
update's `fadd double %r11, 1.0` becomes `add i32 %r12, 1`. Selecting
canonical-i32 *moves the storage*, so unlike `Ptr<Shape>` there is no slot left
for an unconsumed selection to fall back to: every read and write of the local
is forced through the i32 slot or it does not compile.

**Census** (`compiler_output_regression.py census`): corpus-wide `canonical-i32`
**13 → 17**. No floor dropped; `fixture_canonical_slots` rises 2 → 3 (its
`u32Mixer` counter now promotes). The new liveness fixture
`fixture_loop_bounded_i32` has no bitwise mixing, no `| 0` and no array
indexing, so its three `canonical-i32` promotions can only come from this rule;
its `LIVENESS_FLOORS` minimum is pinned at 3 in code, where `--update` cannot
reach it.

**Wider sweep** (201 parsed files: 200 `test_gap_*.ts` + the app-pattern
kernels), canonical-slot verdicts before → after:

| | before | after |
|---|---|---|
| selected `I32` | 24 | **28** |
| denied `not_index_used_or_bounded` | 77 | **51** |
| denied `module_init_context` | 127 | **145** |
| denied `closure_referenced` / `declared_bigint` | 7 / 5 | 7 / 5 |

The 26 locals that leave `not_index_used_or_bounded` split 4 promoted / 18 now
blocked *only* by the module-init context gate (#7109) / 4 that were being
reported twice and are now one selection. So the rule proves **22** more locals
than it promotes today; the other 18 land the moment #7109 is fixed.

**gc-ratchet** (`--repeats 7`, `shared_ci`): OK. The gated retention and
evacuation counters are **bit-identical** between `PERRY_CANONICAL_I32_LOCALS`
on and off — the representation change moves nothing the collector counts. The
ungated `wall_ms` column moves on 7 of 8 probes (+1.2% to +8.2% slower with the
canonical model off), which is what proves the two arms were different binaries
rather than one stale archive measured twice.
