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
   `repsel_context_allows_ptr_shape: false` for module-init and program-entry
   bodies, and `FnCtx::ptr_shape_receiver_fact` returns `None` for the whole
   body when that flag is clear. Every access site falls back to the guarded
   diamond.

   That flag used to be `repsel_context_allows_canonical_i32`, shared with a
   different representation. #7109 lifted the entry-body exclusion for
   canonical i32/u32/Str and split the flag rather than dragging `Ptr<Shape>`
   along with it: #6991 is an open rooting bug for a compiled receiver held
   across the `globalThis`-population collection, which runs around module
   init. So this row is unchanged, and it is now the only representation the
   rule names.
2. **`async_body` / `generator_body`** (#6328) — the canonical flag, cleared for
   a different reason. `Ptr<Shape>` reads the same rule name through its own
   flag.
3. **`scalar_replaced`** (#7115) — `collectors/escape_news.rs` deleted the
   object outright. This one is the *better* outcome, not a defect; it is listed
   because "scalar-replaced" and "promoted but wasted" used to render
   identically and mean opposite things.

**Ground truth is the emitted IR, never a counter.** Every verdict above is
reproducible without the report at all: compile the workload twice, once with
`PERRY_PTR_SHAPE_LOCALS=0`, and compare the objects.

`--no-link` does **not** honour `-o`: the object goes to a per-run temp
directory and the path is printed. So capture the printed path in each arm and
compare those — comparing the `-o` arguments compares two files that were never
created.

```bash
obj() {  # echo the object path this compile actually wrote
  "$@" --no-link --no-cache 2>&1 | sed -n 's/^Wrote object file: //p'
}
a=$(obj perry compile <src> -o /tmp/ignored)
b=$(PERRY_PTR_SHAPE_LOCALS=0 obj perry compile <src> -o /tmp/ignored)
cmp "$a" "$b" && echo "IDENTICAL — the promotion emitted nothing"
```

Byte-identical objects mean the promotions the report counted as wins changed
nothing. `07_object_create` and `12_binary_trees` are byte-identical today.
`09_method_calls` differs, but only by two `__pshape` clones with **zero call
sites** — which is why the census reports its consumption as 0 and the object
A/B alone would have been misleading.

### Per-site coverage

The consumed count is per **value**, so one working recorder is enough to mark a
value consumed and the other five could rot unnoticed. `CONSUMPTION_SITES` (in
the script) registers all six, and a site that records nothing corpus-wide fails
the run.

This is not hypothetical. When coverage was first measured, four fired and two
had **never fired on any workload here** —
`class_field_get_number.shape_proven_load` and `ptr_shape_update`. Both were
reachable; nothing reached them. `fixture_ptr_shape_sites.ts` exists to.

Only `ptr-shape` has consumption instrumentation. The other seven census keys
report *no consumption data* rather than a zero
(`CONSUMPTION_INSTRUMENTED` in the script), because "uninstrumented" and "never
applied" are exactly the pair this census exists to keep apart.

## The one check that catches an EXTRA promotion

Every number above is a promotion count and every gate on it is a **floor**, so
the census can only ever go red when a representation stops firing. #7128 is the
opposite failure: `benchmarks/suite/15_mandelbrot.ts` promoted three counters it
should not have — all three provably i32-bounded, none ever used as an integer —
and paid **+14.87% instructions retired** for them, measured on a quiet
Raspberry Pi 5 at a 0.02% noise floor. No floor in this file can go red for
that; more promotions always reads as an improvement.

So the deliberate refusal gets a minimum of its own. `REFUSAL_FLOORS` (in
`scripts/compiler_output_harness/repsel_census.py`, in code and not in the
baseline, for the same reason as `LIVENESS_FLOORS`) says how many times the
`no_i32_consuming_use` rule must fire per workload. Deleting
`crates/perry-codegen/src/collectors/repsel_benefit.rs` takes `15_mandelbrot`
from three refusals to zero and the census red.

`fixture_loop_bounded_i32.ts` carries the paired case: `iterate()`'s counter and
`mixedWithFloat()`'s counter are admitted by the identical #7110 interval proof
and differ only in what consumes them. One must promote (its `canonical-i32`
liveness floor) and one must be refused (its refusal floor), so neither an
always-yes nor an always-no rule can satisfy the file.

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
   `codegen/entry.rs` excluded module-init contexts from canonical selection
   before any per-value rule ran (#7109). Nothing in the census could have told
   the difference; the follow-up recorded those as denials so it could, and
   #7109 then removed the exclusion — the same values are now selections, and
   `canonical-i32` went from promoting in 2 of 18 real workloads to 17 of 18.

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

## Knob isolation (#7128)

The census answers "how much does each representation promote". It cannot, on
its own, answer "is knob X evidence about representation X" — and for two of the
five knobs it was not:

- `PERRY_CANONICAL_I32_LOCALS=0` also turned off **every `Ptr<Shape>`
  consumption**, because the four ordinary-body `FnCtx` construction sites
  computed the `Ptr<Shape>` context flag from the canonical-i32 env read. Census
  under that knob read `ptr-shape: 7 selected, 0 consumed`. On this corpus it
  moved the object on `batch`, `suite_09_method_calls` and
  `fixture_ptr_shape_sites` for `Ptr<Shape>` reasons alone.
- `PERRY_CANONICAL_STR_LOCALS=0` also turned off three lowerings that never
  consult a selected `Str` local, so it changed the emitted object on **23 of
  the 26** workloads — 20 of which promote no `canonical-str` at all.

```bash
python3 scripts/compiler_output_regression.py census-knob-isolation \
    --perry <path/to/perry> --jobs 4
```

Per knob, with that knob at `0` and every other at its default:

1. no census key outside the knob's own may change;
2. a workload whose representation promotes nothing must emit a
   **byte-identical** object;
3. the knob must still be live — take a promotion away somewhere, and change
   some object.

Rule 1 catches the first defect, rule 2 the second (it leaves every count
untouched). Two controls guard the diff: the compiler must be deterministic, and
both `X=1` and an env var the compiler does not read must reproduce the default
object bit-for-bit. A determinism failure aborts the run — it is not a host
property to route around (see below).

## Emission determinism (#7131)

Every object comparison on this page assumes the compiler is a function of its
inputs. On ELF it was not: the temp `.ll` name carried pid + wall-clock nanos,
and clang records a translation unit's **source basename** into the object as an
`STT_FILE` symbol, so two identical compiles differed by exactly those digits —
26/26 census workloads on a Raspberry Pi 5, 12 bytes apart on `suite_01_startup`:

```
run1  STT_FILE  perry_llvm_217502_1785528949373123236_0.ll
run2  STT_FILE  perry_llvm_217533_1785528951945773193_0.ll
```

Mach-O does not record that name in the `.o` at all, which is why macOS looked
clean while carrying the same defect — and why this cannot be reviewed on a Mac.
#7135 content-addressed the `.ll` basename.

Check it before trusting any object-level result on a host you have not measured
on (the whole corpus twice is ~7 s):

```bash
python3 scripts/compiler_output_regression.py census-determinism \
    --perry <path/to/perry> --repeat 2 --jobs 4
```

Repeats run **concurrently** on purpose. Identical IR now shares one
content-addressed `.ll`, so racing it is part of the subject — and that is what
caught #7135's other half, where the `.o` name lost its pid and two `perry`
processes compiling the same file deleted each other's object.

The knob-isolation gate runs the same control inline and **fails** on a
disagreement. It used to skip its emission half instead, which meant the half
that caught the `PERRY_CANONICAL_STR_LOCALS` defect could not run on Linux at
all — the host where object-hash A/B is most useful, because it is the one with
an unprivileged instruction-retired counter.

One documented exception, downward only: `PERRY_INT_VALUED_LOCALS=0` lowers
`canonical-i32` on `fixture_int_valued_ta` (3 → 2), because
`int_valued_ta_locals` is merged into `integer_locals`, the candidate set
canonical-i32 draws from. A withdrawn proof cannot be selected. A knob that
*raises* another representation's count is still a leak.

`--jobs` compiles arms in parallel; the whole corpus × 6 knobs × 4 arms is about
20 s on an M1.

## Temp-directory hygiene (#7144)

The other consequence of content-addressing that `.ll`: two workers holding
identical IR now *share* the path, so a per-call unlink can race a sibling that
computed the path but has not yet handed it to clang. #7135 responded by not
deleting it — and then nothing did. The leftovers are bounded by **distinct IR
ever compiled** on the machine, which is fine in CI (runner temp dirs are
reclaimed) and not fine anywhere a compiler is being worked on, where the IR
changes on every rebuild: 1627 files / 951.8 MB after one day on one dev box,
~29 GB on another.

#7144 removed the sharing rather than the deletion. Each `.ll` → `.o` compile
gets a private directory under the temp root and removes it on success; the
*basename* inside it is still a pure function of the IR, so the determinism
property above is untouched — the directory is not recorded in the object, only
the basename is.

```bash
python3 scripts/compiler_output_regression.py census-temp-hygiene \
    --perry <path/to/perry> --repeat 2 --jobs 4
```

It compiles the corpus with `TMPDIR` pointed at an empty directory of its own
and asserts the directory is still empty. Two things worth knowing before
changing it:

* **"No growth run-over-run" would have been green on the broken compiler.**
  Compiling the same corpus twice produces the same content-addressed names, so
  a repeat-and-compare check sees a flat count while the machine fills up. The
  property that goes red is the absolute one: *nothing* left behind.
* **The `TMPDIR` isolation is load-bearing**, not politeness. Counting entries
  in the shared system temp dir measures every other process on the box.

It fails on the clang driver's own temp names (`perry_llvm_*`, `perry_cgu_*`,
`perry_bc_*`) and merely *reports* anything else — today that is the compile
driver's `perry-objs-<pid>-<nanos>/` staging directory, which `--no-link` never
cleans up (#7167). Widen `OWNED_PREFIXES` to "everything" once that is closed.

No exemption for `PERRY_DEBUG_SYMBOLS`, and that is a change of belief rather
than a change of policy. `-g` was documented as pulling the `.ll`'s **absolute**
path plus `DW_AT_comp_dir` into DWARF, which would have made the file part of
the shipped object and forced it to persist. Measured on a real Perry module
(Apple clang 21, `-target x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`): the `-g` object is **byte-identical** to the one
without it and has **no `.debug_*` sections at all**. Perry's codegen emits no
`DICompileUnit`/`DIFile`/`!dbg` metadata, and `clang -g` on a `.ll` lowers debug
info that is already in the IR rather than synthesising a compile unit for the
input file. So there is one layout, not two, and
`debug_symbols_do_not_change_what_the_object_records` in
`linker_temp_lifecycle_tests.rs` goes red the day that stops being true. (Not
measured on COFF/Windows.)

That test has a sibling worth knowing about:
`the_ll_directory_is_not_recorded_in_the_object_but_the_basename_is` compiles
one `.ll` under the same basename from two different directories, for both
Linux ELF targets, and asserts the objects are identical — with a control that
a *different* basename does change them. That is the property this whole
directory rests on, and until #7144 it lived only in a comment and a hand
measurement taken once on a Raspberry Pi.

## Editing the fixtures

Don't tidy them. Every one is written against a specific collector's rules and
most of the obvious cleanups disqualify the very local under test — passing a
`Ptr<Shape>` local to a function is an escape (rule 2), wrapping a canonical-i32
local in a loop moves it to the parallel-shadow model, adding a bounds check to
`fixture_int_valued_ta` moves its locals to the ordinary integer-local path.
Each file says which edits would silently take it to zero.

`fixture_module_init_canonical.ts` has one extra rule of its own: **it must
never grow a function, method or closure.** Its whole claim is that every count
it reports came from the module-init `FnCtx`; moving one loop into a helper
would make it a duplicate of `fixture_canonical_slots.ts` and leave the entry
context untested again.

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
