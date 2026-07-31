# Representation-selection promotion census (#7106)

How many values actually get each of Perry's unboxed representations, per
workload, with a ratcheted floor so a drop turns a build red.

```bash
# Count promotions across the corpus and check the floors.
python3 scripts/compiler_output_regression.py census --gate

# Report only, no verdict.
python3 scripts/compiler_output_regression.py census

# Re-record the floors after an intentional change.
python3 scripts/compiler_output_regression.py census --update

# Verdict logic only (no compiler needed).
python3 scripts/compiler_output_regression.py census-self-test
```

## Why this exists

Perry's performance story rests on six unboxed representations. Until #7034,
nobody knew how many values any of them promoted on real code, because nothing
reported it — an agent had to hand-instrument the compiler to find out that
`Ptr<Shape>` promotes **nothing at all** on `benchmarks/app-patterns/kernels/batch.ts`,
the object/property-heavy program that representation exists for.
Independently confirmed at the time: `PERRY_PTR_SHAPE_LOCALS=0` and the default
produced a byte-identical binary.

The census makes that a standing, visible, gated number instead of a discovery.

## Selected is not consumed

The census counts **consumed** promotions, not selected ones, and the two
columns are separate on purpose.

`select()` fires when an analysis *proves* a value. Whether codegen then emits
anything different for it is a different question, and for `Ptr<Shape>` the
answer is usually no:

| workload | `ptr-shape` | `ptr-shape-consumed` | mechanism |
|---|---|---|---|
| `fixture_ptr_shape` | 1 | 1 | — |
| `batch` | 2 | 1 | `module_init_context` |
| `suite_07_object_create` | 1 | 0 | `scalar_replaced` |
| `suite_09_method_calls` | 1 | 0 | `module_init_context` |
| `suite_12_binary_trees` | 1 | 0 | `scalar_replaced` |

Six proven, two applied. A promotion goes unconsumed three ways, and every one
of them is recorded at the site where the proof is dropped:

1. **`module_init_context`** (#7109) — `codegen/entry.rs` sets
   `repsel_context_allows_canonical_i32: false` for module-init and
   program-entry bodies, and `FnCtx::ptr_shape_receiver_fact` returns `None` for
   the whole body when that flag is clear. Every access site falls back to the
   guarded diamond.
2. **`async_body` / `generator_body`** (#6328) — the same flag, cleared for a
   different reason.
3. **`scalar_replaced`** (#7115) — `collectors/escape_news.rs` deleted the
   object outright. This one is the *better* outcome, not a defect; it is listed
   because "scalar-replaced" and "promoted but wasted" used to render
   identically and mean opposite things.

**Ground truth is the emitted IR, never a counter.** Every verdict above is
reproducible without the report at all: compile the workload twice, once with
`PERRY_PTR_SHAPE_LOCALS=0`, and compare the objects.

```bash
perry compile <src> -o /tmp/x --no-link --no-cache          # prints the .o path
PERRY_PTR_SHAPE_LOCALS=0 perry compile <src> -o /tmp/x --no-link --no-cache
```

Byte-identical objects mean the promotions the report counted as wins changed
nothing. `07_object_create` and `12_binary_trees` are byte-identical today.
`09_method_calls` differs, but only by two `__pshape` clones with **zero call
sites** — which is why the census reports its consumption as 0 and the object
A/B alone would have been misleading.

Only `ptr-shape` has consumption instrumentation. The other seven census keys
report *no consumption data* rather than a zero
(`CONSUMPTION_INSTRUMENTED` in the script), because "uninstrumented" and "never
applied" are exactly the pair this census exists to keep apart.

## How it cannot quietly pass

Read CLAUDE.md, "★ Four ways a gate can be unable to fail". The fourth applies
here most directly: *the gate runs but its subject never did*. A census that
faithfully prints `Ptr<Shape>: 0` and exits green is worth nothing.

Three separate mechanisms, in increasing order of paranoia:

1. **Per-workload, per-representation floors** in `baseline.json`. A count
   below its floor is a regression. Counts are recorded per representation,
   never aggregated — the interesting signal is `Ptr<Shape>` being 0 *while*
   canonical `Str` is nonzero, and one total hides exactly that.

2. **Liveness fixtures** in `fixtures/`. Floors alone are not enough: the
   honest floor for `Ptr<Shape>` on real code is zero today, and a zero floor
   can never fail. Each fixture is written to satisfy one representation's
   proof obligations in full, and its minimum lives in `LIVENESS_FLOORS` in
   `scripts/compiler_output_harness/repsel_census.py` — **in code, not in the
   regenerable baseline**, so that re-running `--update` after a breakage
   cannot write the fixture down to zero and leave a permanently-green gate.
   `--update` refuses to do it.

3. **A corpus-wide instrument check**: a census key that reads zero in *every*
   workload fails the run. A counter that is zero because nothing promoted and
   one that is zero because nobody increments it look identical; this
   distinguishes them. `Ptr<NumArray>` was in the second state until #7106 — it
   had an `Analysis` variant, a `Ptr<NumArray>` target-rep string, and no
   `select()` call site anywhere in the tree.

4. **An analysis-reach check**: a corpus workload whose *candidate* total is
   zero across every analysis fails the run, unless it is named in
   `ZERO_CANDIDATE_ALLOWLIST` (in the script, not the baseline) with a reason.

   Mechanisms 1–3 are all about promotion counts, and promotion counts cannot
   see this. "Considered and denied" names a rule you can argue with; "zero
   candidates" names nothing — and the two produce an identical census table.
   When #7104 landed, **8 of the 18 real workloads were in the second state**,
   every one because its hot loop is at module top level and
   `codegen/entry.rs` excludes module-init contexts from canonical selection
   before any per-value rule runs (#7109). Nothing in the census could have
   told the difference; the follow-up records those as denials so it can.

   Only `suite_01_startup` is allowlisted: it is a lone `console.log`, with no
   bindings for any analysis to consider.

5. **Consumption coherence checks.** `consumed` may never exceed `selected` for
   the same representation — they must describe one population, and Phase 5a's
   proven-`this` receiver is consumed without ever being selected, so folding it
   in would silently break that. One value consumed at five access sites counts
   once. And a workload with wasted promotions must NAME at least one mechanism.

   That last one is what makes deleting a drop-recorder visible. Without it the
   consumed column would not move, every floor would still pass, and the census
   would go green having lost the only part of the finding that says *why* —
   CLAUDE.md failure mode 4, one level in.

Sabotage-verified in both directions. Each of `PERRY_PTR_SHAPE_LOCALS=0`,
`PERRY_PTR_NUMARRAY_LOCALS=0`, `PERRY_CANONICAL_I32_LOCALS=0`,
`PERRY_CANONICAL_STR_LOCALS=0` and `PERRY_INT_VALUED_LOCALS=0` turns the census
red; the default build is green. CI re-runs the first of those on every job so
the property is checked, not just claimed once, and additionally asserts that
`ptr-shape-consumed` goes to zero with it — the consumed column is fed from
separate codegen sites and needs its own liveness proof.

The consumption machinery was sabotage-verified the same way: dropping
`outcome` from `Entry::dedup_key`, removing all six consumption recorders,
removing either mechanism recorder, counting per access site, folding
proven-`this` consumption into the local column, and deleting the consumed
liveness minimum each turn the gate red.

## Editing the fixtures

Don't tidy them. Every one is written against a specific collector's rules and
most of the obvious cleanups disqualify the very local under test — passing a
`Ptr<Shape>` local to a function is an escape (rule 2), wrapping a canonical-i32
local in a loop moves it to the parallel-shadow model, adding a bounds check to
`fixture_int_valued_ta` moves its locals to the ordinary integer-local path.
Each file says which edits would silently take it to zero.

If a fixture legitimately stops exercising its representation, change the
fixture *and* say so in the PR. Lowering `LIVENESS_FLOORS` instead is how this
gate would end up unable to fail.

## Coverage

Instrumented (counted by `--opt-report`, #6952):

| Census key | Representation | Source of truth |
| --- | --- | --- |
| `ptr-shape` | `Ptr<Shape>` | `collectors/ptr_shape.rs` |
| `ptr-numarray` | `Ptr<NumArray>` | `collectors/ptr_numarray.rs` |
| `canonical-i32` / `canonical-u32` / `canonical-str` | canonical slot reps | `expr/slot_rep.rs` |
| `int-valued-ta` | int-valued locals | `collectors/int_valued_ta_locals.rs` |
| `spec-abi-entry` | specialized ABI entries | `codegen/typed_abi.rs` |
| `spec-abi-taptr-slot` | `TaPtr` parameter slots | `collectors/spec_abi_sites.rs` |

**Not** instrumented, stated plainly rather than reported as a zero: the
masked-window / buffer-view `TaPtr` *region* machinery
(`stmt/masked_window_region.rs`). It is region-shaped rather than a per-value
promotion and has no `opt_report` analysis, so the census makes no claim about
it. `spec-abi-taptr-slot` covers `TaPtr` only in its parameter form.

### What a zero in the `candidates` column still cannot tell you

The analysis-reach check above is per *workload*, not per *analysis*. A single
analysis can still read zero candidates on a workload for either reason,
because `--opt-report` records inside the per-value rules — a value filtered
out before it becomes a candidate produces no entry:

- `Ptr<NumArray>` admits only `number[]` / `Int32Array`-typed bindings, and
  bails for the whole module on a prototype barrier. `11_prime_sieve`'s
  `boolean[]` sieve is correctly not a candidate, and correctly invisible.
- `Ptr<Shape>` gates provenance before the containment walk that records.
- A function whose call sites were all inlined away never reaches the
  spec-ABI decision loop at all (`14_closure`).

Tracked as #7112 and #7111. Until those land, read a per-analysis zero as
"either nothing of this shape, or a pre-filter", never as a denial.
