# Bisect: the 2026-08-13 broad regression `0a21611fe` → `843ef621f`

**Verdict: `4784d5da7` — #7983, "#6759 C3 rung 1 — make the shape word uniform".**
Single commit, isolated against its own parent. Fixed in PR #8010.

Not #7997 (the aarch64 SVE prologue decoder) and not #7994 (per-thread
prototype addresses) — both landed *after* the regression was already fully
present and cost nothing measurable on this corpus.

## Instrument

Wall clock on the dev box cannot resolve this (load 30–200 all session), so
everything here is **instructions retired** (`/usr/bin/time -l`, best-of-3,
exit-checked, output `cmp`-ed against `m0810/expected/`). Every hop rebuilt
`-p perry -p perry-runtime-static -p perry-stdlib-static` into ONE target dir
with the `.a` mtimes checked. Corpus binaries went to `$HOME/bisect-bins/<sha>/`
with the SAME basenames in DIFFERENT directories, per the protocol's `cmp` trap.

The instruction delta tracks the reported wall-clock delta row for row, which is
what says the regression is **work-bound**.

## The five measured arms (instructions retired, best-of-3)

| bench | `0a21611fe` good | `23a8aad31` parent | `4784d5da7` #7983 | `843ef621f` tip | `1b53332f8` main | **#8010 fixed** |
|---|--:|--:|--:|--:|--:|--:|
| cycles | 1,788,433,524 | 1,788,778,236 | **2,759,621,970** | 2,758,613,331 | 2,758,649,326 | **1,788,432,369** |
| deeplist | 891,705,830 | 898,511,111 | **1,304,689,118** | 1,297,094,745 | 1,303,183,101 | **888,880,890** |
| interp | 11,619,044,867 | 11,616,644,282 | **14,908,419,553** | 14,890,479,866 | 14,893,088,080 | **11,615,285,338** |
| pipeline | 2,584,827,137 | 2,582,965,931 | **3,199,913,725** | 3,192,369,325 | 3,193,275,738 | **2,582,793,215** |
| iso_miss | 14,201,035,818 | 14,205,407,941 | **17,464,543,037** | 17,444,768,294 | 17,459,670,264 | **14,205,774,080** |
| churn | 3,092,382,790 | 3,091,040,177 | 3,127,189,885 | 3,122,830,475 | 3,123,870,835 | 3,088,954,190 |
| retain | 1,903,836,211 | 1,929,069,359 | 1,933,154,142 | 1,901,604,111 | 1,913,067,161 | 1,901,657,815 |
| asyncpipe | 1,205,121,699 | 1,206,723,078 | 1,233,483,901 | 1,231,925,146 | 1,234,911,466 | 1,206,410,762 |
| fib40 | 3,837,818,438 | 3,838,189,564 | 3,839,712,991 | 3,835,231,921 | 3,836,450,763 | 3,838,170,517 |

The whole regression appears **at #7983 and nothing after it adds any** — the
`4784d5da7` column already equals the tip. `cycles` fixed matches the good
endpoint to **1,155 instructions out of 1.79e9 (0.00006%)**.

## Mechanism

The split is **by receiver kind, not program size**: `cycles` (`class Cell`),
`deeplist` (`class LNode`), `interp`, `pipeline`, `iso_miss` regressed;
`churn` and `retain` (`type … = { … }` object literals) did not.

The emitted read PIC (`expr/property_get/generic_dispatch.rs`) derives its whole
cache token from the header shape word:

```
is_stamp = (parent_class_id - 0x8000_0000) u< 0x4000_0000
token    = is_stamp ? (parent_class_id | 1<<62) : keys_array
```

and its own comment states the premise: *"Everything else (class instances,
unstamped receivers) keeps the keys-pointer compare."*

Rung 1 broke that premise **halfway**. It stamps a class instance — but LAZILY,
at the first by-name resolve — while **codegen INLINE-allocates `new C(…)` and
stores a literal `0` into that word**, never calling
`js_object_alloc_class_with_keys` at all. So one shape's population splits, and
at any site reading a field of a freshly allocated instance:

1. instance #1 misses, is stamped, primes the **id** token;
2. instance #2 is newborn, computes the **keys-pointer** token → miss;
3. the handler stamps #2 and re-primes the same id (ids are per keys-array);
4. instance #3 is newborn → miss. Forever. Hit rate **0%**.

### The single-build proof

Three programs, one compiler, differing only in how many read passes they make
over the same 3,000,000-instance array:

| program | instructions | delta | per read |
|---|--:|--:|--:|
| build array only | 2,564,930,818 | — | — |
| build + 1 read pass | 2,695,683,246 | **130,752,428** | **43.6** |
| build + 2 read passes | 2,742,277,736 | **46,594,490** | **15.5** |

Pass 1 sees each instance NEWBORN; pass 2 sees the SAME instances already
stamped. **2.8×, and the only difference is the stamp.**

## The fix that did NOT work (kept because measuring it is what found the truth)

The first attempt birth-stamped in `js_object_alloc_class_with_keys` /
`js_object_alloc_class_dynamic_parent`, reading the memoized
`ShapeCacheEntry::runtime_shape_id`. It measured **zero recovery** — `cycles`
2,759,257,399, unchanged. `--trace llvm` on `cycles.ts` then showed why: the
`new Cell(…)` site is a bump-pointer allocation emitting
`store i64 8589934592` (parent_class_id 0 ‖ field_count 2) directly; the runtime
allocator is declared but never called. Worse, birth-stamping only the runtime
path would have created a NEW split for any class allocated both ways.

★ **A fix whose subject never runs looks exactly like a fix that didn't help.**
The `.a` mtimes moved, the binaries differed, the unit test passed, and the
change was still inert on the hot path.

## ★ #8009 LANDED MID-VALIDATION AND IS THE FIX

`144867bfc` (#8009, C3 rung 2) reached `main` while this was being validated: it
mints a ShapeId per class at module init and stores it in the inline `new C(…)`
allocation, making the population uniformly STAMPED. Measured on current main
(`f58b73f4f`), the regression is **gone** — `cycles` 1,790,326,029 against the
good endpoint's 1,788,433,524 (+0.11%), `interp` +0.51%, `iso_miss` +0.61%.

That obsoleted the second dead end below, and it was caught only because
`git diff origin/main..HEAD` showed a changelog fragment being DELETED — i.e.
main had gained one. **Re-fetch `main` before finishing; a branch cut 90 minutes
ago is not current in this repo.**

### Dead end #2: holding the stamp at plain objects

A `shape_word_is_stampable` predicate restoring the `class_id == 0` gate. It
recovered the corpus completely (`cycles` 1,788,432,369 — the good endpoint to
0.00006%), and it is the WRONG fix on top of #8009: the runtime would refuse to
READ a stamp codegen now writes at birth, priming the keys-pointer token while
the emitted PIC computes the id token — the same 0% hit rate from the opposite
side.

## What #8009 left behind, and what PR #8010 now is

#8009 stamps only the COMPILED entry point,
`js_object_alloc_class_inline_keys_stamped`. Three class-instance allocators are
still on rung 1's lazy self-heal, which its own doc states:

* `js_object_alloc_class_with_keys`
* `js_object_alloc_class_dynamic_parent`
* `js_object_alloc_class_inline_keys` (the compatibility entry point)

For any class reaching one of those the population is **still split**. The gate
test below FAILS on `main` as of #8009 (`left: 2199047503880` — a keys pointer;
`right: 4611686020574871741` — bit 62 | id) and passes once all three
birth-stamp. #8009's own test cannot see it: it asserts a newborn CARRIES a
stamp, which is a presence check that both-stamped and both-unstamped each
satisfy. Only the MIXTURE is the bug.

PR #8010 is therefore: birth-stamp those three, plus the gate, plus these notes.
It is **neutral on this corpus** (all nine programs' classes take the compiled
path #8009 already fixed) — the value is the classes that do not, and the gate.

## Question 2 — churn / retain / asyncpipe were flat; the mini's +13/+13/+27% is not this commit

Peak RSS and GC collection counts are BOTH load-independent, so this is valid on
a busy box. `PERRY_GC_DIAG=1` **and** `PERRY_GC_TRACE=1` (DIAG alone prints
nothing); positive control — the printer emits 445 lines and 22
`collection_kind":"minor"` for `cycles`.

| bench | quantity | `0a21611fe` | `1b53332f8` main | Δ |
|---|---|--:|--:|--:|
| churn | instructions | 3,092,462,749 | 3,124,640,479 | +1.0% |
| | peak RSS (KB) | 24,976 | 24,992 | +0.06% |
| | minors / fulls | 88 / 0 | 88 / 0 | identical |
| retain | instructions | 1,936,036,952 | 1,931,641,207 | −0.2% |
| | peak RSS (KB) | 254,800 | 254,816 | +0.006% |
| | minors / fulls | 4 / 0 | 4 / 0 | identical |
| asyncpipe | instructions | 1,206,387,962 | 1,231,635,504 | +2.1% |
| | peak RSS (KB) | 38,832 | 39,168 | +0.9% |
| | minors / fulls | 1 / 0 | 1 / 0 | identical |
| **cycles** (control) | instructions | 1,790,911,190 | 2,759,606,611 | **+54%** |
| | peak RSS (KB) | 24,816 | 24,800 | −0.06% |
| | minors / fulls | 22 / 0 | 22 / 0 | identical |

* **GC-scheduling explanation: refuted.** Collection counts are identical on all
  four programs, zero full collections anywhere. Nothing was rescheduled.
* **Locality/footprint explanation: refuted.** Peak RSS is flat to ≤0.9%.
* **Work: flat** for churn (+1.0%) and retain (−0.2%).

★ Note the control row: `cycles` regressed **54%** with RSS and collection counts
**also flat**. So flat RSS/counts can only REFUTE the scheduling and footprint
explanations — they can never confirm "nothing changed". The positive statement
for churn/retain is the instruction count, and it is flat.

With no work added, no collection rescheduled and no footprint change, there is
no mechanism left for a 13% wall-clock move on churn or retain: **those two rows
are mini-side variance.** They should regain their node wins on the next sweep —
the fix leaves them 1.1% and 0.6% BELOW main.

`asyncpipe` is the one row with a real attributable cost: **+2.1%**, not +27%,
and the fix returns it exactly to the good endpoint (1,206,410,762 vs
1,205,121,699). The remaining ~25 points are either mini variance or **idle/
parked time**, which neither instructions retired nor cycles elapsed can observe
for an async program — that can only be settled on the quiet mini, and it is now
moot.

## Validation

* `cargo test -p perry-runtime --lib` (`RUST_TEST_THREADS=1`): **2278 pass, 0 fail**.
* Sabotage-verified with the fix **committed first**; restored and **REBUILT**
  (`Compiling perry-runtime` = 1) before re-confirming green.
* `iso_miss` canary prints `checksum 437840 misses 0`, including under
  `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800
  PERRY_GC_VERIFY_EVACUATION=1`; `cycles`/`deeplist`/`interp` byte-identical
  under the same knobs.
* Whole probe corpus output-verified against `m0810/expected/` at every arm.

## Bisect hygiene notes

* `git status` cannot see a stale `.a`. Every hop verified `libperry_runtime.a`
  and `perry` mtimes moved after the checkout.
* The `d456b411e` dirty-but-corroborating sweep recorded in
  MEASUREMENT-PROTOCOL.md held up: everything up to and including #7981 is flat.
