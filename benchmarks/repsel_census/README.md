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

Sabotage-verified in both directions. Each of `PERRY_PTR_SHAPE_LOCALS=0`,
`PERRY_PTR_NUMARRAY_LOCALS=0`, `PERRY_CANONICAL_I32_LOCALS=0`,
`PERRY_CANONICAL_STR_LOCALS=0` and `PERRY_INT_VALUED_LOCALS=0` turns the census
red; the default build is green. CI re-runs the first of those on every job so
the property is checked, not just claimed once.

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
