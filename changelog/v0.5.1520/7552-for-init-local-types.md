**A declaration in a `for` initializer is now typed like any other `let`/`const`,
instead of registering as `Any` (#7547).**

`ctx.define_local(name, Type::Any)` at the for-init declarator sites discarded
both the annotation and the initializer. The cost was never the loop variable
itself — it was everything computed from it. `base + j` infers `Any` the moment
`j` is `Any`, so an object literal in the loop body minted an `__AnonShape_…`
class whose fields were all `Any`.

`Any` is pointer-bearing (`typed_shape::type_is_pointer_bearing`), and that is
where it stops being a missed optimisation:

- the raw-f64 mask came out **empty** and the pointer mask **full**, so a
  `{v: number, w: number}` literal was handed to the collector as two traced
  pointer slots — `GC_LAYOUT_SIDE_MASK`, never `POINTER_FREE`;
- there was no raw-f64 store path for the #5093 class-field fast path to engage;
- #7532's declare-at-allocation gate refused the shape, because it requires an
  empty pointer mask.

For-init declarators now route through the same `infer_decl_type` the ordinary
declaration path uses (annotation first, else the initializer's inferred type,
else the tsgo-resolved fallback). **`var` is deliberately left alone**: it is
function-scoped and hoisted, so it can be written before its declaration runs
and its assignment story is not the same one.

## Measured

Only three benchmarks compile to different code at all; the rest are
**byte-identical generated objects** across the two arms, so their ±2% is noise
by construction rather than by assertion.

| bench | code | speedup |
|---|---|--:|
| `churn_alloc` | changed | **1.197×** |
| `churn` | changed | **1.122×** |
| `churn_read` | changed | 1.000× |
| `push_num`, `push_cls`, `push_cls_read`, `deeplist`, `tree` | identical | — |

The mechanism is directly visible in the collector trace, not inferred:
`churn_alloc`'s `pointer_slots_read` falls **376,504 → 188,456** while
`pointer_free_slots_skipped` rises **0 → 188,048`. The literal's payload is now
skipped instead of scanned. Cycles (105), bytes copied (0.0036 GB), promoted
bytes (64) and peak RSS (24.2 MB) are unchanged.

## GC ratchet

All 12 probes: `correctness` identical to the baseline arm. Every metric that
moved beyond ±5% moved in the improving direction, except one `wall_ms` on a
host at load 33:

| probe | metric | change |
|---|---|--:|
| `03_cross_gen_writes` | `heap_used_bytes` | −48.9% |
| `04_dead_after_deep_stack` | `copied_bytes` | −93.3% |
| `04_dead_after_deep_stack` | `rss_bytes` | −18.6% |
| `03_cross_gen_writes` | `rss_bytes` | −17.1% |

**One thing the ratchet owner should look at rather than bank.**
`03_cross_gen_writes` now promotes **zero** bytes (was 210,700 / 4,752 objects),
and a probe with no old generation has fewer old→young edges to remember. The
probe's *mutator* subject is provably still live — `remembered_set_insert_attempts`
is **491,520 on both arms, identical** — and its correctness assertion still
passes, so this is not a silently vacuous gate. But `remembered_set_marking`
drops 1,344 → 866, so its coverage of the remembered set is genuinely reduced,
and the probe may want a change that forces promotion to keep measuring what it
was written to measure.

## Blast radius

This is a "more type visibility" change and #6377 is the standing lesson for
that class — registering `Number` where `Any` was assumed un-gates latent fast
paths that have never run on the shape before. So the testing is behavioural,
not just compile-level:

- 67 `test-files/` programs spread deterministically across the corpus, compiled
  and run under both arms: **identical output**, and identical again under
  `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`. (Three initial "diffs" were
  thread ids inside a pre-existing panic message and are normalised out; they
  differ run-to-run on the same binary.)
- `perry-hir` and `perry-runtime` suites green; `perry-codegen`'s failure set is
  identical to `origin/main`'s.

**Pre-existing, unrelated, and worth its own issue:**
`test-files/test_gap_webcrypto_async_threadpool.ts` crashes (`Bus error`,
rc=138) under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` on **both** arms,
with nondeterministic output. Not introduced here.
